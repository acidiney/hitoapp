import { expect, test } from "@playwright/test";
import type { components } from "../src/generated";
import { liveAudioRequestDiagnostics } from "./fixtures/youtube-audio-network";
import {
  liveAudioState,
  liveControlBounds,
  liveMediaDiagnostics,
  liveYouTubeConfiguration,
  signInLiveAccount,
} from "./fixtures/youtube-live";

type Schema = components["schemas"];
class LiveFailure extends Error {}
const audioPath =
  /^\/api\/v1\/sources\/[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}\/audio$/;
test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});

test.describe("isolated packaged YouTube", () => {
  test.skip(
    process.env.HITO_YOUTUBE_LIVE !== "1",
    "Requires explicit live-image opt-in",
  );

  test("address-only connection discovers, saves, plays and seeks actual AAC through Hito", async ({
    page,
    baseURL,
    browserName,
  }, info) => {
    test.setTimeout(120_000);
    let stage = "configuration";
    let stageNumber = 0;
    let offered: Schema["PluginService"] | null = null;
    let failure: string | null = null;
    let cleanupFailed = false;
    let pageErrors = 0;
    let externalRequests = 0;
    let invalidMediaRequests = 0;
    let nonzeroRanges = 0;
    let playStage = 0;
    const started = Date.now();
    const next = (name: string) => {
      stage = name;
      stageNumber++;
    };
    const origin = baseURL ? new URL(baseURL).origin : null;
    const audioNetwork = liveAudioRequestDiagnostics(
      page,
      origin,
      () => stageNumber,
    );
    page.on("pageerror", () => pageErrors++);
    page.on("request", (request) => {
      const url = new URL(request.url());
      if (!["http:", "https:"].includes(url.protocol)) return;
      if (url.origin !== origin) externalRequests++;
      if (request.resourceType() !== "media") return;
      if (
        url.origin !== origin ||
        !audioPath.test(url.pathname) ||
        url.search ||
        url.hash
      )
        invalidMediaRequests++;
      if (/^bytes=[1-9][0-9]*-/.test(request.headers().range ?? ""))
        nonzeroRanges++;
    });
    const headers = { "x-hito-request": "1", origin: origin ?? "" };
    try {
      if (!origin)
        throw new LiveFailure(
          "The disposable browser server must be configured",
        );
      const configuration = liveYouTubeConfiguration();
      next("browser AAC capability");
      const capability = await page.evaluate(() =>
        document
          .createElement("audio")
          .canPlayType('audio/mp4; codecs="mp4a.40.2"'),
      );
      if (!capability)
        throw new LiveFailure(
          "This browser does not support the required AAC codec",
        );
      next("disposable account sign-in");
      await signInLiveAccount(page);
      await page
        .getByRole("navigation", { name: "Main navigation" })
        .getByRole("link", { name: "Settings", exact: true })
        .click();
      await page.getByRole("tab", { name: "Plugins", exact: true }).click();
      await expect(
        page.getByRole("heading", { name: "Your plugins", exact: true }),
      ).toBeVisible();

      next("address-only service offer");
      await page.getByText("Services for this server", { exact: true }).click();
      await expect(
        page.getByLabel("Service name", { exact: true }),
      ).toHaveCount(0);
      await expect(
        page.getByLabel("Custom label (optional)", { exact: true }),
      ).toBeHidden();
      await page
        .getByLabel("Service address", { exact: true })
        .fill(configuration.address);
      const offerResponse = page.waitForResponse(
        (response) =>
          new URL(response.url()).pathname === "/api/v1/plugins/services" &&
          response.request().method() === "POST",
      );
      await page
        .getByRole("button", { name: "Offer service", exact: true })
        .click();
      const offer = await offerResponse;
      if (offer.status() !== 201)
        throw new LiveFailure(`Service offer returned HTTP ${offer.status()}`);
      const offerBody = offer
        .request()
        .postDataJSON() as Schema["OfferPluginService"];
      expect(Object.keys(offerBody)).toEqual(["base_url"]);
      expect(offerBody.base_url === configuration.address).toBe(true);
      offered = await offer.json();
      if (!offered)
        throw new LiveFailure(
          "The service offer did not return its declaration",
        );
      expect(offered.name === "YouTube").toBe(true);
      const service = page.getByRole("combobox", {
        name: "Service",
        exact: true,
      });
      await expect(
        service.getByRole("option", { name: "YouTube", exact: true }),
      ).toHaveCount(1);
      await service.selectOption(offered.id);

      next("personal connection without a bearer");
      await expect(
        page.getByLabel("Bearer token (optional)", { exact: true }),
      ).toHaveValue("");
      const installResponse = page.waitForResponse(
        (response) =>
          new URL(response.url()).pathname === "/api/v1/plugins" &&
          response.request().method() === "POST",
      );
      await page
        .getByRole("button", { name: "Connect service", exact: true })
        .click();
      const installed = await installResponse;
      if (installed.status() !== 201)
        throw new LiveFailure(
          `Service connection returned HTTP ${installed.status()}`,
        );
      expect(installed.request().postDataJSON().bearer_token === null).toBe(
        true,
      );
      const connection: Schema["PluginInstallation"] = await installed.json();
      expect(connection.credential_set).toBe(false);
      expect(connection.manifest.name === "YouTube").toBe(true);
      expect(
        connection.manifest.capabilities.includes("search") &&
          connection.manifest.capabilities.includes("sources"),
      ).toBe(true);
      await expect(
        page
          .locator(".plugin-connection")
          .getByText("No token saved", { exact: true }),
      ).toBeVisible();

      next("actual provider discovery");
      await page
        .getByRole("navigation", { name: "Main navigation" })
        .getByRole("link", { name: "Discover", exact: true })
        .click();
      await page
        .getByRole("combobox", { name: "Music service", exact: true })
        .selectOption(connection.id);
      await page
        .getByRole("searchbox", { name: "Search music", exact: true })
        .fill(configuration.video);
      const searchResponse = page.waitForResponse(
        (response) =>
          new URL(response.url()).pathname ===
            `/api/v1/plugins/${connection.id}/search` &&
          response.request().method() === "POST",
      );
      await page.getByRole("button", { name: "Search", exact: true }).click();
      const searched = await searchResponse;
      if (searched.status() !== 200)
        throw new LiveFailure(
          `Provider search returned HTTP ${searched.status()}`,
        );
      const found: Schema["SearchPage"] = await searched.json();
      const index = found.items.findIndex(
        (item) => item.id === configuration.video,
      );
      if (index < 0)
        throw new LiveFailure(
          "Provider search did not return the configured public recording",
        );
      const result = page
        .getByRole("list", { name: "Music search results" })
        .locator(".provider-result")
        .nth(index);
      await expect(result).toBeVisible();
      expect(
        (await result.getByRole("heading", { level: 3 }).textContent()) ===
          found.items[index].title,
      ).toBe(true);
      const add = result.getByRole("button", { name: /^Add .+ to library$/ });
      await expect(add).toBeEnabled();
      await liveControlBounds(page, add, true);

      next("save through the interface");
      const adoptionResponse = page.waitForResponse(
        (response) =>
          new URL(response.url()).pathname ===
            `/api/v1/plugins/${connection.id}/recordings` &&
          response.request().method() === "POST",
      );
      await add.click();
      const adopted = await adoptionResponse;
      if (adopted.status() !== 200)
        throw new LiveFailure(
          `Library addition returned HTTP ${adopted.status()}`,
        );
      expect(
        adopted.request().postDataJSON().provider_recording_id ===
          configuration.video,
      ).toBe(true);
      const recording: Schema["Recording"] = await adopted.json();
      expect(recording.available).toBe(true);
      expect(
        recording.id !== configuration.video &&
          /^[a-f0-9-]{36}$/.test(recording.id),
      ).toBe(true);
      await expect(
        result.getByRole("link", { name: /^View .+ in library$/ }),
      ).toBeVisible();
      const sourceResponse = await page.request.get(
        `/api/v1/recordings/${recording.id}/sources`,
      );
      if (sourceResponse.status() !== 200)
        throw new LiveFailure(
          `Saved source inspection returned HTTP ${sourceResponse.status()}`,
        );
      const sources: Schema["Source"][] = await sourceResponse.json();
      expect(sources.length).toBeGreaterThan(0);
      expect(
        sources.every(
          (source) => source.ready && source.recording_id === recording.id,
        ),
      ).toBe(true);

      next("play saved AAC through the interface");
      playStage = 1;
      await page
        .getByRole("navigation", { name: "Main navigation" })
        .getByRole("link", { name: "Library", exact: true })
        .click();
      playStage = 2;
      await page
        .getByRole("button", { name: "List view", exact: true })
        .click();
      playStage = 3;
      const row = page
        .getByRole("row")
        .filter({ has: page.locator(`a[href="/recordings/${recording.id}"]`) });
      const play = row.getByRole("button", { name: /^Play .+/ });
      await expect(play).toBeEnabled();
      playStage = 4;
      await play.click();
      playStage = 5;
      await expect
        .poll(
          async () => {
            const state = await liveAudioState(page);
            return (
              state.time > 0.3 &&
              state.ready >= 2 &&
              state.error === 0 &&
              !state.paused
            );
          },
          { timeout: 30_000 },
        )
        .toBe(true);
      playStage = 6;
      const first = await liveAudioState(page);
      const current = new URL(first.source);
      expect(
        current.origin === origin &&
          !current.search &&
          !current.hash &&
          audioPath.test(current.pathname),
      ).toBe(true);
      expect(
        sources.some(
          (source) => current.pathname === `/api/v1/sources/${source.id}/audio`,
        ),
      ).toBe(true);
      playStage = 7;
      await expect
        .poll(async () => (await liveAudioState(page)).time, {
          timeout: 10_000,
        })
        .toBeGreaterThan(first.time + 0.2);
      playStage = 8;

      next("seek actual media");
      const seek = page.getByRole("slider", { name: "Seek", exact: true });
      await expect(seek).toBeEnabled();
      const beforeSeek = await liveAudioState(page);
      expect(
        Number.isFinite(beforeSeek.duration) && beforeSeek.duration > 10,
      ).toBe(true);
      await seek.focus();
      await seek.press("PageUp");
      await seek.press("PageUp");
      const target = Number(await seek.inputValue());
      expect(target).toBeGreaterThan(1);
      expect(target).toBeGreaterThan(beforeSeek.time + 1);
      await expect
        .poll(
          async () => {
            const state = await liveAudioState(page);
            return (
              state.time >= target - 0.5 &&
              state.ready >= 2 &&
              state.error === 0 &&
              !state.paused &&
              !state.seeking
            );
          },
          { timeout: 20_000 },
        )
        .toBe(true);
      const afterSeek = await liveAudioState(page);
      expect(afterSeek.time < target + 5).toBe(true);

      next("navigation preserves actual playback");
      await page
        .getByRole("navigation", { name: "Main navigation" })
        .getByRole("link", { name: "History", exact: true })
        .click();
      await expect
        .poll(async () => (await liveAudioState(page)).time, {
          timeout: 10_000,
        })
        .toBeGreaterThan(afterSeek.time + 0.2);
      const afterNavigation = await liveAudioState(page);
      expect(
        afterNavigation.source === first.source &&
          afterNavigation.ready >= 2 &&
          afterNavigation.error === 0 &&
          !afterNavigation.paused,
      ).toBe(true);
      await liveControlBounds(page, seek, false);
      expect(pageErrors).toBe(0);
      expect(externalRequests).toBe(0);
      expect(invalidMediaRequests).toBe(0);
      const network = audioNetwork.summary(browserName);
      expect(network.audio_http_failures).toBe(0);
      expect(network.audio_framing_failures).toBe(0);
      expect(network.audio_unexpected_request_failures).toBe(0);
      expect(network.audio_request_failures).toBe(
        network.audio_browser_cancellations +
          network.audio_unexpected_request_failures,
      );
      expect(network.audio_observation_overflow).toBe(0);
      expect(network.audio_responses).toBeGreaterThan(0);
      const metrics = {
        event: "youtube_live_browser_verified",
        browser_family:
          browserName === "chromium" ? 1 : browserName === "firefox" ? 2 : 3,
        codec_support: capability === "probably" ? 2 : 1,
        first_time: first.time,
        seek_target: target,
        seek_time: afterSeek.time,
        navigated_time: afterNavigation.time,
        ready_state: afterNavigation.ready,
        media_error: afterNavigation.error,
        ...network,
        audio_requests: audioNetwork.snapshot(),
        nonzero_range_requests: nonzeroRanges,
        external_requests: externalRequests,
        invalid_media_requests: invalidMediaRequests,
        page_errors: pageErrors,
        elapsed_ms: Date.now() - started,
      };
      await info.attach("youtube-live-numerical-evidence", {
        body: Buffer.from(JSON.stringify(metrics)),
        contentType: "application/json",
      });
      console.info(JSON.stringify(metrics));
      await page
        .getByRole("contentinfo", { name: "Audio player" })
        .getByRole("button", { name: "Pause", exact: true })
        .click();
    } catch (error) {
      failure = `YouTube live workflow failed at ${stage}: ${error instanceof LiveFailure ? error.message : "UI or media assertion failed"}`;
      let media: Record<string, number> = { diagnostic_available: 0 };
      try {
        media = {
          diagnostic_available: 1,
          ...(await liveMediaDiagnostics(page)),
        };
      } catch {
        /* Preserve the original failure without raw browser diagnostics. */
      }
      console.info(
        JSON.stringify({
          event: "youtube_live_browser_failed",
          stage: stageNumber,
          page_errors: pageErrors,
          external_requests: externalRequests,
          invalid_media_requests: invalidMediaRequests,
          ...audioNetwork.summary(browserName),
          audio_requests: audioNetwork.snapshot(),
          play_stage: playStage,
          ...media,
        }),
      );
    } finally {
      audioNetwork.markCleanup();
      if (offered) {
        try {
          const removed = await page.request.delete(
            `/api/v1/plugins/services/${offered.id}`,
            {
              headers,
              data: { revision: offered.revision },
            },
          );
          cleanupFailed = removed.status() !== 204;
        } catch {
          cleanupFailed = true;
        }
      }
    }
    if (failure) throw new Error(failure);
    if (cleanupFailed)
      throw new Error(
        "The disposable live service could not be withdrawn during cleanup",
      );
  });
});
