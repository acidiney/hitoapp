import { expect, test, type Locator, type Page } from "@playwright/test";
import type { components } from "../src/generated";
import { captureConsolePrivacy } from "./fixtures/console-privacy";
import {
  connectLiveProvider,
  setUpSpotifyYouTubeLiveAccount,
  spotifyYouTubeConfiguration,
} from "./fixtures/spotify-youtube-live";
import {
  inspectCopiedPlaylist,
  inspectImportedRecording,
  readSpotifyYouTube as read,
  uuid,
} from "./fixtures/spotify-youtube-collection";
import {
  assertAutomaticLiveCollection,
  seedAutomaticLiveState,
} from "./fixtures/spotify-youtube-automatic";
import { liveAudioState } from "./fixtures/youtube-live";
import { audioPath } from "./fixtures/spotify-youtube-playback";

type Schema = components["schemas"];
type Phase =
  | "configuration"
  | "setup"
  | "connections"
  | "import"
  | "private_state"
  | "single_track"
  | "automatic_resolution"
  | "aac_playback"
  | "seek"
  | "navigation"
  | "preservation"
  | "privacy"
  | "cleanup";
const optedIn = process.env.HITO_SPOTIFY_YOUTUBE_AUTOMATIC_E2E === "1";

test.use({
  browserName: "webkit",
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});
test.skip(
  !optedIn,
  "Requires an opted-in fresh Spotify to YouTube automatic server",
);

async function clickReceipt<T>(
  page: Page,
  control: Locator,
  path: string,
  timeout: number,
): Promise<T> {
  const pending = page
    .waitForResponse(
      (response) =>
        new URL(response.url()).pathname === path &&
        response.request().method() === "POST",
      { timeout },
    )
    .catch(() => null);
  let clicked = false;
  try {
    await control.click();
    clicked = true;
  } catch {}
  const response = await pending;
  expect(clicked && response !== null && response.status() === 200).toBe(true);
  if (response === null)
    throw new Error("An accepted action receipt is required");
  return response.json();
}

test("ordinary Play resolves one imported Spotify recording to real YouTube AAC without manual matching or persistent downloads", async ({
  page,
  context,
  baseURL,
  browserName,
}) => {
  test.setTimeout(240_000);
  let phase: Phase = "configuration";
  let failed: Phase | null = null;
  const started = Date.now();
  const counts = {
    imports: 0,
    connections: 0,
    automatic: 0,
    manual: 0,
    downloads: 0,
    audioRequests: 0,
    audioResponses: 0,
    audioInvalid: 0,
    audioZero: 0,
    external: 0,
    pageErrors: 0,
    callbackFailures: 0,
    privateLogs: 0,
    textCaptures: 0,
    snapshotCaptures: 0,
    consoleCanary: 0,
    logOverflow: 0,
    cleanupFailures: 0,
  };
  const pending = new Set<Promise<void>>();
  const logs: string[] = [];
  const privateValues = [
    "Owned private automatic listening label",
    "Owned private automatic artist",
    "docker browser test password",
  ];
  const [personalTitle, personalArtist] = privateValues;
  const resolutionRequests: { path: string; body: string }[] = [];
  let acceptedResolution: { path: string; body: string } | null = null;
  let origin = "";
  let playedTime = 0;
  let seekTime = 0;
  let finalTime = 0;

  try {
    if (
      !baseURL ||
      browserName !== "webkit" ||
      process.env.HITO_YOUTUBE_LIVE !== "1" ||
      process.env.PLAYWRIGHT_NO_COPY_PROMPT !== "1"
    )
      throw new Error(
        "Explicit live and private WebKit configuration is required",
      );
    const server = new URL(baseURL);
    if (
      server.protocol !== "http:" ||
      server.port !== "8080" ||
      !["localhost", "127.0.0.1", "e2e-server"].includes(server.hostname) ||
      server.username ||
      server.password ||
      server.search ||
      server.hash ||
      server.pathname !== "/"
    )
      throw new Error("A locally isolated disposable server is required");
    origin = server.origin;
    const configuration = spotifyYouTubeConfiguration();
    privateValues.push(
      configuration.playlist,
      configuration.youtube,
      configuration.spotify,
    );
    page.setDefaultTimeout(10_000);
    page.setDefaultNavigationTimeout(10_000);
    await captureConsolePrivacy(
      context,
      (text, capture) => {
        if (capture === "text") counts.textCaptures++;
        else counts.snapshotCaptures++;
        if (
          capture === "snapshot" &&
          text.includes("hito_owned_automatic_live_canary_v1")
        )
          counts.consoleCanary++;
        if (logs.length < 1024) logs.push(text);
        else counts.logOverflow++;
      },
      () => counts.callbackFailures++,
      pending,
    );
    context.on("page", (opened) =>
      opened.on("pageerror", () => counts.pageErrors++),
    );
    page.on("pageerror", () => counts.pageErrors++);
    context.on("request", (request) => {
      try {
        const target = new URL(request.url());
        if (target.protocol.startsWith("http") && target.origin !== origin)
          counts.external++;
        if (target.origin !== origin) return;
        if (audioPath.test(target.pathname)) {
          counts.audioRequests++;
          if (
            target.search ||
            target.hash ||
            !["GET", "HEAD"].includes(request.method())
          )
            counts.audioInvalid++;
        }
        if (request.method() !== "POST") return;
        if (target.pathname === "/api/v1/plugins") counts.connections++;
        if (/\/playlists\/import$/.test(target.pathname)) counts.imports++;
        if (/\/sources\/resolve$/.test(target.pathname)) {
          counts.automatic++;
          const body: Schema["ResolveSource"] = request.postDataJSON();
          if (
            Object.keys(body).join(",") !== "request_id" ||
            !uuid.test(body.request_id)
          )
            counts.callbackFailures++;
          resolutionRequests.push({
            path: target.pathname,
            body: JSON.stringify(body),
          });
        }
        if (
          /\/plugins\/[^/]+\/(search|recordings)$/.test(target.pathname) ||
          /\/sources\/(review|attach|prepare)$/.test(target.pathname) ||
          /\/source-choice(?:\/reset)?$/.test(target.pathname)
        )
          counts.manual++;
        if (
          /\/sources\/refresh$/.test(target.pathname) &&
          request.postDataJSON()?.purpose !== "playback"
        )
          counts.manual++;
        if (/\/download$/.test(target.pathname)) counts.downloads++;
      } catch {
        counts.callbackFailures++;
      }
    });
    context.on("response", (response) => {
      try {
        const target = new URL(response.url());
        if (target.origin !== origin || !audioPath.test(target.pathname))
          return;
        if (response.status() === 0) {
          counts.audioZero++;
          return;
        }
        const headers = response.headers();
        if (
          [200, 206].includes(response.status()) &&
          /^audio\/mp4(?:\s*;|$)/i.test(headers["content-type"] ?? "") &&
          /^[0-9]+$/.test(headers["content-length"] ?? "") &&
          Number(headers["content-length"]) > 0
        )
          counts.audioResponses++;
        else counts.audioInvalid++;
      } catch {
        counts.callbackFailures++;
      }
    });

    phase = "setup";
    await setUpSpotifyYouTubeLiveAccount(page);
    await page.evaluate(() =>
      console.info({
        event: "owned_console_probe",
        nested: { value: "hito_owned_automatic_live_canary_v1" },
      }),
    );
    await expect.poll(() => counts.consoleCanary).toBeGreaterThan(0);
    expect(
      await page.evaluate(
        () => new Audio().canPlayType('audio/mp4; codecs="mp4a.40.2"') !== "",
      ),
    ).toBe(true);
    phase = "connections";
    const spotify = await connectLiveProvider(
      page,
      configuration.spotify,
      "Spotify public playlists",
      () => {},
    );
    const youtube = await connectLiveProvider(
      page,
      configuration.youtube,
      "YouTube",
      () => {},
    );
    expect(
      spotify.manifest.capabilities.includes("playlists") &&
        !spotify.manifest.capabilities.includes("sources") &&
        youtube.manifest.id === "org.hito.youtube" &&
        ["search", "sources", "metadata"].every((capability) =>
          youtube.manifest.capabilities.some((item) => item === capability),
        ),
    ).toBe(true);
    const connection = page
      .getByRole("listitem")
      .filter({
        has: page.getByRole("heading", { name: "YouTube", exact: true }),
      })
      .filter({
        has: page.getByRole("button", {
          name: "Check connection",
          exact: true,
        }),
      });
    await expect(connection.getByText(/^Declared capabilities:/)).toContainText(
      "metadata",
    );

    phase = "import";
    await page
      .getByRole("navigation", { name: "Main navigation" })
      .getByRole("link", { name: "Playlists", exact: true })
      .click();
    const form = page.getByRole("region", {
      name: "Import a playlist",
      exact: true,
    });
    await form
      .getByRole("combobox", { name: "Playlist service", exact: true })
      .selectOption(spotify.id);
    await form
      .getByRole("textbox", {
        name: "Playlist link or identifier",
        exact: true,
      })
      .fill(configuration.playlist);
    const receipt = await clickReceipt<Schema["PlaylistImportReceipt"]>(
      page,
      form.getByRole("button", { name: "Import a copy", exact: true }),
      `/api/v1/plugins/${spotify.id}/playlists/import`,
      60_000,
    );
    expect(uuid.test(receipt.playlist_id)).toBe(true);
    await expect
      .poll(
        () =>
          new URL(page.url()).pathname === `/playlists/${receipt.playlist_id}`,
      )
      .toBe(true);
    const { copied, recording } = await inspectCopiedPlaylist(
      page,
      receipt,
      spotify,
    );
    const original = await inspectImportedRecording(page, recording, spotify);
    privateValues.push(
      copied.playlist.name,
      original.title,
      original.artist,
      original.provider_recording_id ?? "",
    );

    phase = "private_state";
    const before = await seedAutomaticLiveState(
      page,
      origin,
      recording,
      receipt.playlist_id,
      personalTitle,
      personalArtist,
    );
    expect(before.metadata.selected_observation?.id === original.id).toBe(true);
    expect(
      counts.audioRequests +
        counts.automatic +
        counts.manual +
        counts.downloads,
    ).toBe(0);
    expect(
      await page.locator("audio[data-hito-player]").getAttribute("src"),
    ).toBeNull();

    phase = "single_track";
    await page
      .getByRole("navigation", { name: "Main navigation" })
      .getByRole("link", { name: "Library", exact: true })
      .click();
    const search = page.getByRole("textbox", {
      name: "Search library",
      exact: true,
    });
    await search.fill(personalTitle);
    await search.press("Enter");
    const table = page.getByRole("table", { name: "Tracks", exact: true });
    await expect(table.getByRole("row")).toHaveCount(2);
    const found = await read<Schema["RecordingPage"]>(
      page,
      `/api/v1/recordings?q=${encodeURIComponent(personalTitle)}`,
    );
    expect(found.total === 1 && found.items[0].id === recording).toBe(true);
    expect(counts.audioRequests + counts.automatic).toBe(0);

    phase = "automatic_resolution";
    const resolution = await clickReceipt<Schema["SourceResolution"]>(
      page,
      table.getByRole("button", { name: `Play ${personalTitle}`, exact: true }),
      `/api/v1/recordings/${recording}/sources/resolve`,
      20_000,
    );
    acceptedResolution = {
      path: `/api/v1/recordings/${recording}/sources/resolve`,
      body: JSON.stringify({ request_id: resolution.request_id }),
    };
    expect(
      resolution.outcome === "ready" &&
        resolution.recording_id === recording &&
        resolutionRequests.length >= 1 &&
        resolutionRequests.length <= 2 &&
        resolutionRequests.every(
          (request) =>
            request.path ===
              `/api/v1/recordings/${recording}/sources/resolve` &&
            request.body ===
              JSON.stringify({ request_id: resolution.request_id }),
        ),
    ).toBe(true);

    phase = "aac_playback";
    await expect
      .poll(
        async () => {
          const state = await liveAudioState(page);
          return (
            state.time > 0.3 &&
            !state.paused &&
            state.ready >= 2 &&
            state.error === 0
          );
        },
        { timeout: 30_000 },
      )
      .toBe(true);
    const first = await liveAudioState(page);
    const source = new URL(first.source);
    const playedSource = source.pathname.split("/")[4];
    privateValues.push(first.source);
    expect(
      source.origin === origin &&
        !source.search &&
        !source.hash &&
        audioPath.test(source.pathname) &&
        uuid.test(playedSource) &&
        Number.isFinite(first.duration) &&
        first.duration > 10 &&
        first.duration <= 600 &&
        Math.abs(first.duration * 1000 - original.duration_ms) <=
          Math.min(10_000, Math.max(3000, original.duration_ms * 0.02)) + 1000,
    ).toBe(true);
    await expect
      .poll(async () => (await liveAudioState(page)).time, { timeout: 10_000 })
      .toBeGreaterThan(first.time + 0.2);
    playedTime = (await liveAudioState(page)).time;

    phase = "seek";
    const dock = page.getByRole("contentinfo", {
      name: "Audio player",
      exact: true,
    });
    await dock.getByRole("button", { name: "Pause", exact: true }).click();
    const seek = dock.getByRole("slider", { name: "Seek", exact: true });
    await expect(seek).toBeEnabled();
    await seek.focus();
    await seek.press("Home");
    for (let step = 0; step < 50; step++) await seek.press("ArrowRight");
    expect(Number(await seek.inputValue())).toBeCloseTo(5, 1);
    await dock.getByRole("button", { name: "Play", exact: true }).click();
    await expect
      .poll(
        async () => {
          const state = await liveAudioState(page);
          return (
            state.time > 5.3 &&
            !state.paused &&
            !state.seeking &&
            state.error === 0
          );
        },
        { timeout: 20_000 },
      )
      .toBe(true);
    seekTime = (await liveAudioState(page)).time;

    phase = "navigation";
    await page
      .getByRole("navigation", { name: "Main navigation" })
      .getByRole("link", { name: "History", exact: true })
      .click();
    await expect
      .poll(async () => (await liveAudioState(page)).time, { timeout: 10_000 })
      .toBeGreaterThan(seekTime + 0.2);
    const navigated = await liveAudioState(page);
    expect(
      navigated.source === first.source &&
        !navigated.paused &&
        navigated.error === 0,
    ).toBe(true);
    await dock.getByRole("button", { name: "Pause", exact: true }).click();
    finalTime = (await liveAudioState(page)).time;

    phase = "preservation";
    const preserved = await assertAutomaticLiveCollection(
      page,
      recording,
      receipt.playlist_id,
      youtube,
      before,
      playedSource,
      resolution,
      finalTime,
    );
    privateValues.push(preserved.added[0].provider_recording_id ?? "");
    phase = "privacy";
    expect(
      counts.imports === 1 &&
        counts.connections === 2 &&
        counts.automatic >= 1 &&
        counts.audioResponses > 0 &&
        counts.audioInvalid === 0 &&
        counts.manual + counts.downloads + counts.external === 0,
    ).toBe(true);
    await expect(
      page.getByRole("region", { name: "Review source match", exact: true }),
    ).toHaveCount(0);
  } catch {
    failed = phase;
  } finally {
    try {
      await context.close();
    } catch {
      counts.cleanupFailures++;
    }
    await Promise.allSettled(Array.from(pending));
    counts.privateLogs = logs.filter((text) =>
      privateValues.some((value) => value.length >= 3 && text.includes(value)),
    ).length;
    if (
      counts.privateLogs +
        counts.pageErrors +
        counts.callbackFailures +
        counts.logOverflow >
      0
    )
      failed ??= "privacy";
    if (
      counts.textCaptures === 0 ||
      counts.snapshotCaptures === 0 ||
      counts.consoleCanary === 0
    )
      failed ??= "privacy";
    if (
      counts.imports !== 1 ||
      counts.connections !== 2 ||
      counts.automatic < 1 ||
      counts.automatic > 2 ||
      counts.audioResponses === 0 ||
      counts.audioInvalid + counts.manual + counts.downloads + counts.external >
        0 ||
      acceptedResolution === null ||
      resolutionRequests.some(
        (request) =>
          request.path !== acceptedResolution?.path ||
          request.body !== acceptedResolution?.body,
      )
    )
      failed ??= "privacy";
    if (counts.cleanupFailures > 0) failed ??= "cleanup";
    console.info(
      JSON.stringify({
        event: "spotify_youtube_automatic_live_result",
        failed_phase: failed,
        duration_ms: Date.now() - started,
        ...counts,
        played_ms: Math.round(playedTime * 1000),
        seek_ms: Math.round(seekTime * 1000),
        final_ms: Math.round(finalTime * 1000),
      }),
    );
  }
  if (failed !== null) {
    const failure = new Error(
      `Spotify to YouTube automatic check failed at phase ${failed}`,
    );
    failure.stack = failure.message;
    throw failure;
  }
});
