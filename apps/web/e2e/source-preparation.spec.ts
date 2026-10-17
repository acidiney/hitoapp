import { expect, test as baseTest, type Route } from "@playwright/test";
import type { components } from "../src/generated";
import { sourcePreparationFixture } from "./fixtures/source-preparation";
import { setUpSpotifyYouTubeLiveAccount as setUpDisposableAccount } from "./fixtures/spotify-youtube-live";

type Schema = components["schemas"];
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const optedIn = process.env.HITO_SOURCE_PREPARATION_E2E === "1";
const test = baseTest.extend<{ sourcePreparationPrivacy: void }>({
  sourcePreparationPrivacy: [
    async ({}, use) => {
      if (!optedIn) {
        await use();
        return;
      }
      const previous = process.env.PLAYWRIGHT_NO_COPY_PROMPT;
      process.env.PLAYWRIGHT_NO_COPY_PROMPT = "1";
      try {
        await use();
      } finally {
        if (previous === undefined)
          delete process.env.PLAYWRIGHT_NO_COPY_PROMPT;
        else process.env.PLAYWRIGHT_NO_COPY_PROMPT = previous;
      }
    },
    { auto: true },
  ],
});

test.use({ trace: "off", screenshot: "off", video: "off" });
test.skip(
  !optedIn,
  "Requires an opted-in fresh disposable source preparation server",
);

test("an unavailable source recovers its exact request receipt and plays only after an explicit action", async ({
  page,
  context,
  baseURL,
}) => {
  test.setTimeout(90_000);
  let fixture: Awaited<ReturnType<typeof sourcePreparationFixture>> | null =
    null;
  let phase = "configuration";
  let failedPhase: string | null = null;
  let callbackFailures = 0;
  let cleanupFailures = 0;
  let pageErrors = 0;
  let privateLogs = 0;
  let browserAudio = 0;
  let downloadWrites = 0;
  let preparationPath = "";
  const requests: { body: string; request: string }[] = [];
  const lostReceipts: Schema["SourcePreparationReceipt"][] = [];
  const closePrivatePage = async () => {
    if (page.isClosed()) return;
    try {
      await page.close({ runBeforeUnload: false });
    } catch {
      cleanupFailures++;
      try {
        await context.close();
      } catch {
        cleanupFailures++;
      }
    }
  };
  try {
    if (!baseURL || process.env.PLAYWRIGHT_NO_COPY_PROMPT !== "1")
      throw new Error("The disposable preparation configuration is required");
    const configured = new URL(baseURL);
    if (
      configured.protocol !== "http:" ||
      !["localhost", "127.0.0.1", "e2e-server"].includes(configured.hostname)
    )
      throw new Error("The preparation server must be isolated locally");
    const origin = configured.origin;
    phase = "provider_fixture";
    const provider = await sourcePreparationFixture();
    fixture = provider;
    page.on("pageerror", () => pageErrors++);
    page.on("console", (message) => {
      try {
        if (
          provider.privateValues.some((value) => message.text().includes(value))
        )
          privateLogs++;
      } catch {
        callbackFailures++;
      }
    });
    context.on("request", (request) => {
      try {
        const url = new URL(request.url());
        if (url.origin !== origin) return;
        if (/^\/api\/v1\/sources\/[0-9a-f-]+\/audio$/.test(url.pathname))
          browserAudio++;
        if (
          request.method() === "POST" &&
          /^\/api\/v1\/(recordings|playlists)\/[0-9a-f-]+\/download$/.test(
            url.pathname,
          )
        )
          downloadWrites++;
        if (request.method() === "POST" && url.pathname === preparationPath) {
          const value: Schema["PrepareSource"] = request.postDataJSON();
          requests.push({
            body: request.postData() ?? "",
            request: value.request_id,
          });
        }
      } catch {
        callbackFailures++;
      }
    });
    phase = "setup";
    await setUpDisposableAccount(page);
    const headers = { "x-hito-request": "1", origin };
    const offered = await page.request.post("/api/v1/plugins/services", {
      headers,
      data: {
        name: "Owned preparation service",
        base_url: provider.address,
      } satisfies Schema["OfferPluginService"],
    });
    expect(offered.status()).toBe(201);
    const service: Schema["PluginService"] = await offered.json();
    const installed = await page.request.post("/api/v1/plugins", {
      headers,
      data: {
        service_id: service.id,
        service_revision: service.revision,
        bearer_token: provider.token,
      } satisfies Schema["InstallPlugin"],
    });
    expect(installed.status()).toBe(201);
    const connection: Schema["PluginInstallation"] = await installed.json();
    expect(
      connection.manifest.capabilities.includes("sources") &&
        connection.manifest.capabilities.includes("source_preparation"),
    ).toBe(true);
    const adopted = await page.request.post(
      `/api/v1/plugins/${connection.id}/recordings`,
      {
        headers,
        data: {
          installation_revision: connection.revision,
          provider_recording_id: provider.recording.id,
        } satisfies Schema["AdoptProviderRecording"],
      },
    );
    expect(adopted.status()).toBe(200);
    const recording: Schema["Recording"] = await adopted.json();
    expect(uuid.test(recording.id) && !recording.available).toBe(true);
    const sourcesPath = `/api/v1/recordings/${recording.id}/sources`;
    const initialSources = await page.request.get(sourcesPath);
    expect(initialSources.status()).toBe(200);
    const before: Schema["Source"][] = await initialSources.json();
    expect(
      before.length === 1 &&
        uuid.test(before[0].id) &&
        before[0].recording_id === recording.id &&
        !before[0].ready,
    ).toBe(true);
    const source = before[0].id;
    preparationPath = `${sourcesPath}/${source}/preparation`;
    const downloadPath = `/api/v1/recordings/${recording.id}/download`;
    await page.goto(`/recordings/${recording.id}`);
    const section = page.getByRole("region", {
      name: "Audio availability",
      exact: true,
    });
    const audio = page.locator("audio");
    phase = "availability";
    await section
      .getByRole("button", { name: "Check availability", exact: true })
      .click();
    await expect(
      section.getByText(provider.label, { exact: true }),
    ).toBeVisible();
    const prepare = section.getByRole("button", {
      name: "Prepare audio",
      exact: true,
    });
    await expect(prepare).toBeEnabled();
    expect(provider.counts.begin).toBe(0);
    expect(provider.counts.status).toBe(0);
    expect(provider.counts.audio).toBe(0);
    expect(browserAudio).toBe(0);
    expect(downloadWrites).toBe(0);

    phase = "lost_acknowledgement";
    let committed = false;
    const routePattern = `**${preparationPath}`;
    const loseAcknowledgement = async (route: Route) => {
      try {
        if (route.request().method() !== "POST" || committed) {
          await route.continue();
          return;
        }
        const actual = await route.fetch();
        expect(actual.status()).toBe(202);
        lostReceipts.push(await actual.json());
        committed = true;
        await actual.dispose();
        await route.abort("failed");
      } catch {
        callbackFailures++;
        try {
          await route.abort("failed");
        } catch {
          callbackFailures++;
        }
      }
    };
    await page.route(routePattern, loseAcknowledgement);
    await prepare.click();
    const retry = section.getByRole("button", {
      name: "Retry preparation request",
      exact: true,
    });
    await expect(retry).toBeEnabled();
    expect(
      committed && lostReceipts.length === 1 && requests.length === 1,
    ).toBe(true);
    const receipt = lostReceipts[0];
    expect(
      uuid.test(requests[0].request) &&
        requests[0].request !== "00000000-0000-0000-0000-000000000000" &&
        receipt.request_id === requests[0].request &&
        uuid.test(receipt.preparation_id) &&
        receipt.recording_id === recording.id &&
        receipt.source_id === source,
    ).toBe(true);
    await expect.poll(() => provider.counts.begin, { timeout: 10_000 }).toBe(1);
    expect(provider.operation() === receipt.preparation_id).toBe(true);
    await page.unroute(routePattern, loseAcknowledgement);

    phase = "receipt_recovery";
    const replayResponse = page
      .waitForResponse(
        (response) =>
          new URL(response.url()).pathname === preparationPath &&
          response.request().method() === "POST",
      )
      .catch(() => null);
    await retry.click();
    const replay = await replayResponse;
    if (replay === null) throw new Error("The replay response must arrive");
    expect(replay.status()).toBe(202);
    const replayed: Schema["SourcePreparationReceipt"] = await replay.json();
    expect(JSON.stringify(replayed) === JSON.stringify(receipt)).toBe(true);
    expect(
      requests.length === 2 &&
        requests[1].request === requests[0].request &&
        requests[1].body === requests[0].body,
    ).toBe(true);
    await expect(
      section.getByText("Preparing audio…", { exact: true }),
    ).toBeVisible();
    const progress = section.getByRole("progressbar");
    await expect(progress).toBeVisible();
    expect(
      await progress.evaluate((element) => {
        if (element instanceof HTMLProgressElement)
          return element.value / element.max === 0.25;
        return (
          Number(element.getAttribute("aria-valuenow")) /
            Number(element.getAttribute("aria-valuemax")) ===
          0.25
        );
      }),
    ).toBe(true);
    const preparingResponse = await page.request.get(preparationPath);
    expect(preparingResponse.status()).toBe(200);
    const preparing: Schema["SourcePreparationStatus"] =
      await preparingResponse.json();
    expect(
      preparing.preparation_id === receipt.preparation_id &&
        preparing.recording_id === recording.id &&
        preparing.source_id === source &&
        preparing.phase === "preparing" &&
        preparing.progress_basis_points === 2500,
    ).toBe(true);
    expect(provider.counts.begin).toBe(1);
    expect(provider.counts.audio).toBe(0);
    expect(browserAudio).toBe(0);
    expect(await audio.getAttribute("src")).toBeNull();
    expect(
      await audio.evaluate((element: HTMLAudioElement) => element.paused),
    ).toBe(true);

    phase = "ready";
    provider.releaseReady();
    await expect(
      section.getByText("Audio is ready to play.", { exact: true }),
    ).toBeVisible({ timeout: 40_000 });
    const play = section.getByRole("button", {
      name: "Play track",
      exact: true,
    });
    await expect(play).toBeEnabled();
    const readySources = await page.request.get(sourcesPath);
    expect(readySources.status()).toBe(200);
    const after: Schema["Source"][] = await readySources.json();
    expect(
      after.length === 1 &&
        after[0].id === source &&
        after[0].recording_id === recording.id &&
        after[0].ready,
    ).toBe(true);
    expect((await page.request.get(downloadPath)).status()).toBe(404);
    expect(provider.counts.begin).toBe(1);
    expect(provider.counts.status).toBeGreaterThan(0);
    expect(provider.counts.audio).toBe(0);
    expect(browserAudio).toBe(0);
    expect(downloadWrites).toBe(0);
    expect(requests.length).toBe(2);
    expect(await audio.getAttribute("src")).toBeNull();
    expect(
      await audio.evaluate((element: HTMLAudioElement) => element.paused),
    ).toBe(true);
    const rendered = await section.innerText();
    expect(
      provider.privateValues.some((value) => rendered.includes(value)),
    ).toBe(false);

    phase = "explicit_playback";
    await play.click();
    await expect
      .poll(() =>
        audio.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(0.2);
    expect(
      await audio.evaluate((element: HTMLAudioElement) => element.paused),
    ).toBe(false);
    const playing = new URL(
      await audio.evaluate((element: HTMLAudioElement) => element.src),
    );
    expect(
      playing.origin === origin &&
        playing.pathname === `/api/v1/sources/${source}/audio`,
    ).toBe(true);
    expect(browserAudio).toBeGreaterThan(0);
    expect(provider.counts.audio).toBeGreaterThan(0);
    expect(provider.counts.begin).toBe(1);
    expect(provider.counts.invalid).toBe(0);
    expect(downloadWrites).toBe(0);
    expect(pageErrors).toBe(0);
    expect(privateLogs).toBe(0);
    expect(callbackFailures).toBe(0);
  } catch {
    failedPhase = phase;
  } finally {
    await closePrivatePage();
    try {
      await fixture?.close();
    } catch {
      cleanupFailures++;
      failedPhase ??= "provider_cleanup";
    }
    if (callbackFailures > 0) failedPhase ??= "browser_callback";
    if (cleanupFailures > 0) failedPhase ??= "browser_cleanup";
  }
  if (failedPhase !== null) {
    const failure = new Error(
      `Source preparation check failed at phase ${failedPhase}`,
    );
    failure.stack = failure.message;
    throw failure;
  }
});
