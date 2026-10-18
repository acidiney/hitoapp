import { expect, test, type Locator, type Page } from "@playwright/test";
import type { components } from "../src/generated";
import { captureConsolePrivacy } from "./fixtures/console-privacy";
import { nextEntryFixture } from "./fixtures/next-entry";

type Schema = components["schemas"];
type Phase =
  | "configuration"
  | "fresh_setup"
  | "approved_fixture"
  | "playlist_import"
  | "adopt_first_source"
  | "read_first_source"
  | "read_next_sources"
  | "quiet_before_play"
  | "first_audio_playing"
  | "next_search_before_current_end"
  | "current_audio_during_held_search"
  | "next_audio_buffered_before_current_end"
  | "natural_transition"
  | "preservation_and_privacy"
  | "cleanup";

test.use({
  launchOptions: { args: [] },
  trace: "off",
  screenshot: "off",
  video: "off",
});
test.skip(
  process.env.HITO_NEXT_ENTRY_E2E !== "1",
  "Requires an opted-in fresh disposable host sharing the fixture loopback network",
);

async function read<T>(page: Page, path: string): Promise<T> {
  const response = await page.request.get(path, { timeout: 8000 });
  expect(response.status()).toBe(200);
  return response.json();
}

async function playing(audio: Locator, source: string) {
  return audio.evaluate(
    (element: HTMLAudioElement, source) => ({
      correct: element.getAttribute("src") === source,
      playing: !element.paused && !element.ended && element.error === null,
      position: element.currentTime,
      duration: element.duration,
    }),
    source,
  );
}

test("playlist Play prepares the next distinct recording while current audio plays, then advances without matching again or downloading", async ({
  page,
  context,
  baseURL,
}) => {
  test.setTimeout(75_000);
  page.setDefaultTimeout(8_000);
  page.setDefaultNavigationTimeout(8_000);
  let phase: Phase = "configuration";
  let failedPhase: Phase | null = null;
  let fixture: Awaited<ReturnType<typeof nextEntryFixture>> | null = null;
  let firstPlaybackProven = false;
  let firstStillPlayingAtFailure = false;
  let firstPath = "";
  let nextRecording = "";
  let resolutionWrites = 0;
  let firstAdmissionStatus = 0;
  let firstAdmissionIdentity = false;
  let firstSourcesStatus = 0;
  let firstSourceCount = -1;
  let firstReadyCount = -1;
  let nextSourcesStatus = 0;
  let nextReadyCount = -1;
  let downloadWrites = 0;
  let manualWrites = 0;
  let foreignRequests = 0;
  let pageErrors = 0;
  let privacyFailures = 0;
  let leakedConsole = 0;
  let callbackFailures = 0;
  let cleanupFailures = 0;
  const consoleProbe = { text: 0, snapshot: 0 };
  const pendingConsole = new Set<Promise<void>>();
  const pendingAudio = new Set<Promise<void>>();
  const audioBodies: { path: string; bytes: Buffer; contentRange?: string }[] =
    [];
  const password = "owned next-entry browser password";
  const audio = page.locator("audio[data-hito-player]");

  try {
    if (!baseURL || process.env.PLAYWRIGHT_NO_COPY_PROMPT !== "1")
      throw new Error(
        "The isolated next-entry privacy configuration is required",
      );
    const target = new URL(baseURL);
    if (
      target.protocol !== "http:" ||
      !["localhost", "127.0.0.1"].includes(target.hostname) ||
      target.port !== "8080" ||
      target.pathname !== "/" ||
      target.username ||
      target.password ||
      target.search ||
      target.hash
    )
      throw new Error("The next-entry server must be isolated on loopback");
    const origin = target.origin;
    phase = "fresh_setup";
    expect(await read(page, "/api/v1/setup")).toEqual({ needs_setup: true });
    fixture = await nextEntryFixture();
    const provider = fixture;
    page.on("pageerror", () => pageErrors++);
    await captureConsolePrivacy(
      context,
      (text, capture) => {
        if (text.includes("owned-next-entry-console-probe"))
          consoleProbe[capture]++;
        if (
          [password, ...provider.privateValues].some((value) =>
            text.includes(value),
          )
        )
          leakedConsole++;
      },
      () => privacyFailures++,
      pendingConsole,
    );
    await context.route("**/*", async (route) => {
      const url = new URL(route.request().url());
      if (url.protocol.startsWith("http") && url.origin !== origin) {
        foreignRequests++;
        await route.abort();
      } else await route.continue();
    });
    context.on("request", (request) => {
      try {
        const url = new URL(request.url());
        if (url.origin !== origin || ["GET", "HEAD"].includes(request.method()))
          return;
        if (
          url.pathname === `/api/v1/recordings/${nextRecording}/sources/resolve`
        )
          resolutionWrites++;
        if (/\/downloads?(?:\/|$)/.test(url.pathname)) downloadWrites++;
        if (/\/sources\/(review|attach)$/.test(url.pathname)) manualWrites++;
      } catch {
        callbackFailures++;
      }
    });
    context.on("response", (response) => {
      try {
        const url = new URL(response.url());
        if (
          url.origin !== origin ||
          !/^\/api\/v1\/sources\/[0-9a-f-]+\/audio$/.test(url.pathname)
        )
          return;
        if (response.request().method() === "HEAD") return;
        if (
          ![200, 206].includes(response.status()) ||
          !response.headers()["content-type"]?.startsWith("audio/wav") ||
          audioBodies.length + pendingAudio.size >= 32
        ) {
          callbackFailures++;
          return;
        }
        const checking = response
          .body()
          .then((bytes) => {
            if (bytes.length > 1_000_000) {
              callbackFailures++;
              return;
            }
            audioBodies.push({
              path: url.pathname,
              bytes,
              contentRange: response.headers()["content-range"],
            });
          })
          .catch(() => {
            callbackFailures++;
          })
          .finally(() => pendingAudio.delete(checking));
        pendingAudio.add(checking);
      } catch {
        callbackFailures++;
      }
    });

    await page.goto("/library");
    await page.evaluate(() => console.info("owned-next-entry-console-probe"));
    await page.getByLabel("Username", { exact: true }).fill("admin");
    await page.getByLabel("Password", { exact: true }).fill(password);
    await page
      .getByRole("button", { name: "Create administrator", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();

    phase = "approved_fixture";
    const headers = { "x-hito-request": "1", origin };
    const offered = await page.request.post("/api/v1/plugins/services", {
      headers,
      data: {
        name: "Owned next-entry service",
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

    phase = "playlist_import";
    await page.goto("/playlists");
    const form = page.getByRole("region", {
      name: "Import a playlist",
      exact: true,
    });
    await form
      .getByRole("combobox", { name: "Playlist service", exact: true })
      .selectOption(connection.id);
    await form
      .getByRole("textbox", {
        name: "Playlist link or identifier",
        exact: true,
      })
      .fill(provider.reference);
    const importing = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname ===
          `/api/v1/plugins/${connection.id}/playlists/import` &&
        response.request().method() === "POST",
    );
    await form
      .getByRole("button", { name: "Import a copy", exact: true })
      .click();
    const imported = await importing;
    expect(imported.status()).toBe(200);
    const receipt: Schema["PlaylistImportReceipt"] = await imported.json();
    await expect
      .poll(() => new URL(page.url()).pathname)
      .toBe(`/playlists/${receipt.playlist_id}`);
    const before = await read<Schema["PlaylistDetail"]>(
      page,
      `/api/v1/playlists/${receipt.playlist_id}`,
    );
    const firstRecording = before.entries[0]?.recording_id;
    nextRecording = before.entries[1]?.recording_id ?? "";
    if (
      !firstRecording ||
      !nextRecording ||
      firstRecording === nextRecording ||
      before.entries.length !== 2
    )
      throw new Error("Two distinct saved recordings are required");

    phase = "adopt_first_source";
    const admitted = await page.request.post(
      `/api/v1/plugins/${connection.id}/recordings`,
      {
        headers,
        data: {
          installation_revision: connection.revision,
          provider_recording_id: provider.first.id,
        } satisfies Schema["AdoptProviderRecording"],
      },
    );
    firstAdmissionStatus = admitted.status();
    expect(firstAdmissionStatus).toBe(200);
    const admittedRecording: Schema["Recording"] = await admitted.json();
    firstAdmissionIdentity = admittedRecording.id === firstRecording;
    expect(firstAdmissionIdentity).toBe(true);

    phase = "read_first_source";
    const firstSourcesResponse = await page.request.get(
      `/api/v1/recordings/${firstRecording}/sources`,
      { timeout: 8000 },
    );
    firstSourcesStatus = firstSourcesResponse.status();
    expect(firstSourcesStatus).toBe(200);
    const firstSources: Schema["Source"][] = await firstSourcesResponse.json();
    firstSourceCount = firstSources.length;
    firstReadyCount = firstSources.filter((source) => source.ready).length;
    expect(firstReadyCount).toBe(1);
    firstPath = `/api/v1/sources/${firstSources.find((source) => source.ready)!.id}/audio`;

    phase = "read_next_sources";
    const nextSourcesResponse = await page.request.get(
      `/api/v1/recordings/${nextRecording}/sources`,
      { timeout: 8000 },
    );
    nextSourcesStatus = nextSourcesResponse.status();
    expect(nextSourcesStatus).toBe(200);
    const nextSources: Schema["Source"][] = await nextSourcesResponse.json();
    nextReadyCount = nextSources.filter((source) => source.ready).length;
    expect(nextReadyCount).toBe(0);

    phase = "quiet_before_play";
    expect(
      provider.counts.search +
        provider.counts.firstAudio +
        provider.counts.nextAudio,
    ).toBe(0);

    phase = "first_audio_playing";
    await page
      .getByRole("button", { name: "Play playlist", exact: true })
      .click();
    await expect
      .poll(async () => {
        const current = await playing(audio, firstPath);
        return (
          current.correct &&
          current.playing &&
          current.position > 0.25 &&
          Math.abs(current.duration - 20) < 0.1
        );
      })
      .toBe(true);
    await expect
      .poll(() =>
        audioBodies.some(
          (body) =>
            body.path === firstPath &&
            provider.matchesAudio("first", body.bytes, body.contentRange),
        ),
      )
      .toBe(true);
    firstPlaybackProven = true;
    const firstEnded = audio
      .evaluate(
        (element: HTMLAudioElement) =>
          new Promise<{ natural: boolean; interruptions: number }>(
            (resolve) => {
              const source = element.src;
              let interruptions = 0;
              const interrupted = () => {
                if (
                  !Number.isFinite(element.duration) ||
                  element.currentTime < element.duration - 0.05
                )
                  interruptions++;
              };
              const finish = (natural: boolean) => {
                clearTimeout(timer);
                element.removeEventListener("pause", interrupted);
                element.removeEventListener("emptied", interrupted);
                element.removeEventListener("error", interrupted);
                element.removeEventListener("ended", ended, true);
                resolve({ natural, interruptions });
              };
              const ended = () =>
                finish(
                  element.src === source &&
                    element.currentTime >= element.duration - 0.05,
                );
              const timer = setTimeout(() => finish(false), 23_000);
              element.addEventListener("pause", interrupted);
              element.addEventListener("emptied", interrupted);
              element.addEventListener("error", interrupted);
              element.addEventListener("ended", ended, {
                capture: true,
                once: true,
              });
            },
          ),
      )
      .catch(() => ({ natural: false, interruptions: 1 }));

    phase = "next_search_before_current_end";
    await expect
      .poll(() => provider.searchEntered, { timeout: 3500 })
      .toBe(true);
    expect(provider.counts.search).toBe(1);
    expect(resolutionWrites).toBe(1);
    phase = "current_audio_during_held_search";
    const heldAt = await playing(audio, firstPath);
    expect(heldAt.correct && heldAt.playing).toBe(true);
    await expect
      .poll(
        async () => {
          const current = await playing(audio, firstPath);
          expect(current.correct && current.playing).toBe(true);
          return current.position - heldAt.position;
        },
        { timeout: 2000 },
      )
      .toBeGreaterThan(0.5);
    expect(provider.counts.nextAudio).toBe(0);
    provider.releaseSearch();

    phase = "next_audio_buffered_before_current_end";
    let nextPath = "";
    await expect
      .poll(
        async () => {
          const sources = await read<Schema["Source"][]>(
            page,
            `/api/v1/recordings/${nextRecording}/sources`,
          );
          const ready = sources.find((source) => source.ready);
          if (ready) nextPath = `/api/v1/sources/${ready.id}/audio`;
          return ready !== undefined;
        },
        { timeout: 5000 },
      )
      .toBe(true);
    await expect
      .poll(
        () =>
          audioBodies.some(
            (body) =>
              body.path === nextPath &&
              body.bytes.length >= 32044 &&
              (!body.contentRange ||
                body.contentRange.startsWith("bytes 0-")) &&
              provider.matchesAudio("next", body.bytes, body.contentRange),
          ),
        { timeout: 5000 },
      )
      .toBe(true);
    const bufferedAt = await playing(audio, firstPath);
    expect(
      bufferedAt.correct && bufferedAt.playing && bufferedAt.position < 19,
    ).toBe(true);

    phase = "natural_transition";
    expect(await firstEnded).toEqual({ natural: true, interruptions: 0 });
    await expect
      .poll(
        async () => {
          const current = await playing(audio, nextPath);
          return (
            current.correct &&
            current.playing &&
            current.position > 0.25 &&
            Math.abs(current.duration - 8) < 0.1
          );
        },
        { timeout: 5000 },
      )
      .toBe(true);
    expect(provider.counts.search).toBe(1);
    expect(resolutionWrites).toBe(1);

    phase = "preservation_and_privacy";
    const after = await read<Schema["PlaylistDetail"]>(
      page,
      `/api/v1/playlists/${receipt.playlist_id}`,
    );
    expect(
      after.entries.map((entry) => [entry.entry_id, entry.recording_id]),
    ).toEqual(
      before.entries.map((entry) => [entry.entry_id, entry.recording_id]),
    );
    for (const recording of [firstRecording, nextRecording])
      expect(
        (
          await page.request.get(`/api/v1/recordings/${recording}/download`)
        ).status(),
      ).toBe(404);
    expect(
      (
        await page.request.get(
          `/api/v1/playlists/${receipt.playlist_id}/download`,
        )
      ).status(),
    ).toBe(404);
    await Promise.all([...pendingAudio, ...pendingConsole]);
    expect(
      audioBodies.every((body) =>
        body.path === firstPath
          ? provider.matchesAudio("first", body.bytes, body.contentRange)
          : body.path === nextPath &&
            provider.matchesAudio("next", body.bytes, body.contentRange),
      ),
    ).toBe(true);
    expect(consoleProbe.text > 0 && consoleProbe.snapshot > 0).toBe(true);
    expect(
      downloadWrites +
        manualWrites +
        foreignRequests +
        pageErrors +
        privacyFailures +
        leakedConsole +
        callbackFailures +
        provider.counts.protocol +
        provider.counts.unexpected +
        provider.counts.handler,
    ).toBe(0);
  } catch {
    failedPhase = phase;
    if (firstPlaybackProven && !page.isClosed()) {
      try {
        const current = await playing(audio, firstPath);
        firstStillPlayingAtFailure = current.correct && current.playing;
      } catch {
        callbackFailures++;
      }
    }
  } finally {
    try {
      if (!page.isClosed()) await page.close({ runBeforeUnload: false });
    } catch {
      cleanupFailures++;
    }
    try {
      await context.close();
    } catch {
      cleanupFailures++;
    }
    await Promise.all([...pendingAudio, ...pendingConsole]);
    try {
      await fixture?.close();
    } catch {
      cleanupFailures++;
    }
    if (cleanupFailures) failedPhase ??= "cleanup";
    if (
      callbackFailures +
      privacyFailures +
      leakedConsole +
      foreignRequests +
      pageErrors
    )
      failedPhase ??= "preservation_and_privacy";
    if (
      fixture &&
      fixture.counts.protocol +
        fixture.counts.unexpected +
        fixture.counts.handler
    )
      failedPhase ??= "preservation_and_privacy";
  }
  if (failedPhase) {
    const failure = new Error(
      `Next-entry check failed at phase ${failedPhase}; first_admission_status=${firstAdmissionStatus}; first_admission_identity=${firstAdmissionIdentity}; first_sources_status=${firstSourcesStatus}; first_sources=${firstSourceCount}; first_ready=${firstReadyCount}; next_sources_status=${nextSourcesStatus}; next_ready=${nextReadyCount}; first_details=${fixture?.counts.firstDetails ?? 0}; first_source_reads=${fixture?.counts.firstSources ?? 0}; fixture_protocol=${fixture?.counts.protocol ?? 0}; fixture_unexpected=${fixture?.counts.unexpected ?? 0}; fixture_handler=${fixture?.counts.handler ?? 0}; first_playback_proven=${firstPlaybackProven}; first_still_playing=${firstStillPlayingAtFailure}; next_searches=${fixture?.counts.search ?? 0}`,
    );
    failure.stack = failure.message;
    throw failure;
  }
});
