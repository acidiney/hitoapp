import { expect, type Page } from "@playwright/test";
import type { components } from "../../src/generated";

type Schema = components["schemas"];

export class SpotifyYouTubeLiveFailure extends Error {}

export async function installSpotifyYouTubeMediaDiagnostics(
  page: Page,
  started: number,
) {
  await page.addInitScript((startedAt) => {
    const target = window as typeof window & {
      __hitoLiveMediaEvents?: Record<string, number>[];
    };
    const events: Record<string, number>[] = [];
    target.__hitoLiveMediaEvents = events;
    const kinds = [
      "loadedmetadata",
      "durationchange",
      "canplay",
      "playing",
      "seeking",
      "seeked",
      "waiting",
      "stalled",
      "suspend",
      "abort",
      "error",
      "emptied",
      "ended",
      "progress",
      "pause",
      "timeupdate",
    ];
    for (const [index, kind] of kinds.entries()) {
      document.addEventListener(
        kind,
        (event) => {
          const audio = event.target;
          if (
            !(audio instanceof HTMLAudioElement) ||
            !audio.matches("audio[data-hito-player]")
          )
            return;
          const last = audio.buffered.length - 1;
          events.push({
            kind: index + 1,
            at_ms: Date.now() - startedAt,
            current_time: Number.isFinite(audio.currentTime)
              ? audio.currentTime
              : -1,
            duration: Number.isFinite(audio.duration) ? audio.duration : -1,
            ready_state: audio.readyState,
            network_state: audio.networkState,
            paused: Number(audio.paused),
            seeking: Number(audio.seeking),
            media_error: audio.error?.code ?? 0,
            buffered_ranges: audio.buffered.length,
            buffered_start: last >= 0 ? audio.buffered.start(0) : -1,
            buffered_end: last >= 0 ? audio.buffered.end(last) : -1,
          });
          if (events.length > 256) events.shift();
        },
        true,
      );
    }
  }, started);
}

export async function spotifyYouTubeMediaEvents(page: Page) {
  return page.evaluate(() => {
    const target = window as typeof window & {
      __hitoLiveMediaEvents?: Record<string, number>[];
    };
    return (target.__hitoLiveMediaEvents ?? []).map((event) => ({ ...event }));
  });
}

export function spotifyYouTubeConfiguration() {
  const rawPlaylist = process.env.HITO_SPOTIFY_TEST_PLAYLIST;
  const rawYouTube = process.env.HITO_YOUTUBE_LIVE_BASE_URL;
  if (!rawPlaylist || !rawYouTube)
    throw new SpotifyYouTubeLiveFailure(
      "The opted-in test requires its isolated YouTube service and public Spotify playlist configuration",
    );
  let playlist: URL;
  let youtube: URL;
  try {
    playlist = new URL(rawPlaylist);
    youtube = new URL(rawYouTube);
  } catch {
    throw new SpotifyYouTubeLiveFailure("The live configuration is invalid");
  }
  if (
    playlist.protocol !== "https:" ||
    playlist.hostname !== "open.spotify.com" ||
    playlist.port ||
    playlist.username ||
    playlist.password ||
    playlist.hash ||
    !/^\/playlist\/[A-Za-z0-9]{22}$/.test(playlist.pathname) ||
    youtube.protocol !== "http:" ||
    youtube.hostname !== "plugin-youtube" ||
    youtube.port !== "8093" ||
    youtube.pathname !== "/" ||
    youtube.username ||
    youtube.password ||
    youtube.search ||
    youtube.hash
  )
    throw new SpotifyYouTubeLiveFailure(
      "The test requires the approved isolated YouTube alias and a public Spotify playlist link",
    );
  playlist.search = "";
  return {
    spotify: "http://plugin-spotify:8092/",
    youtube: youtube.href,
    playlist: playlist.href,
  };
}

export async function readSpotifyYouTube<T>(
  page: Page,
  path: string,
): Promise<T> {
  const response = await page.request.get(path);
  if (response.status() !== 200)
    throw new SpotifyYouTubeLiveFailure(
      `Collection inspection returned HTTP ${response.status()}`,
    );
  return response.json();
}

export async function connectLiveProvider(
  page: Page,
  address: string,
  name: string,
  offered: (service: Schema["PluginService"]) => void,
) {
  await page
    .getByRole("navigation", { name: "Main navigation" })
    .getByRole("link", { name: "Settings", exact: true })
    .click();
  await page.getByRole("tab", { name: "Plugins", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Your plugins", exact: true }),
  ).toBeVisible();
  if (!(await page.getByLabel("Service address", { exact: true }).isVisible()))
    await page.getByText("Services for this server", { exact: true }).click();
  await expect(page.getByLabel("Service name", { exact: true })).toHaveCount(0);
  await expect(
    page.getByLabel("Custom label (optional)", { exact: true }),
  ).toBeHidden();
  await page.getByLabel("Service address", { exact: true }).fill(address);
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
    throw new SpotifyYouTubeLiveFailure(
      `Service offer returned HTTP ${offer.status()}`,
    );
  const body: Schema["OfferPluginService"] = offer.request().postDataJSON();
  expect(Object.keys(body).join(",") === "base_url").toBe(true);
  expect(body.base_url === address).toBe(true);
  const service: Schema["PluginService"] = await offer.json();
  offered(service);
  expect(service.name === name).toBe(true);
  await page
    .getByRole("combobox", { name: "Service", exact: true })
    .selectOption(service.id);
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
    throw new SpotifyYouTubeLiveFailure(
      `Personal connection returned HTTP ${installed.status()}`,
    );
  expect(installed.request().postDataJSON().bearer_token === null).toBe(true);
  const connection: Schema["PluginInstallation"] = await installed.json();
  expect(!connection.credential_set && connection.manifest.name === name).toBe(
    true,
  );
  return connection;
}

function normalized(value: string) {
  return value
    .normalize("NFKD")
    .replace(/\p{M}/gu, "")
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim();
}

export function conservativeCandidate(
  original: Schema["MetadataObservation"],
  items: Schema["ProviderRecording"][],
) {
  const title = normalized(original.title);
  const artist = normalized(original.artist)
    .split(" ")
    .filter((word) => word.length >= 2);
  const variants = [
    "cover",
    "karaoke",
    "remix",
    "live",
    "instrumental",
    "sped up",
    "slowed",
    "nightcore",
    "reaction",
    "8d",
  ];
  if (
    title.length < 3 ||
    !artist.length ||
    original.duration_ms <= 10_000 ||
    original.duration_ms > 600_000
  )
    throw new SpotifyYouTubeLiveFailure(
      "The imported track needs bounded, identifiable metadata for explicit review",
    );
  const ranked = items
    .map((candidate, index) => ({ candidate, index }))
    .filter(({ candidate }) => {
      const candidateTitle = normalized(candidate.title);
      const combined = `${candidateTitle} ${normalized(candidate.artist)}`;
      return (
        /^[A-Za-z0-9_-]{11}$/.test(candidate.id) &&
        ` ${candidateTitle} `.includes(` ${title} `) &&
        artist.every((word) => combined.split(" ").includes(word)) &&
        candidate.duration_ms != null &&
        candidate.duration_ms > 10_000 &&
        candidate.duration_ms <= 600_000 &&
        Math.abs(candidate.duration_ms - original.duration_ms) <=
          Math.max(10_000, original.duration_ms * 0.08) &&
        variants.every(
          (variant) =>
            !` ${candidateTitle} `.includes(` ${variant} `) ||
            ` ${title} `.includes(` ${variant} `),
        )
      );
    })
    .sort(
      (left, right) =>
        Math.abs(left.candidate.duration_ms! - original.duration_ms) -
        Math.abs(right.candidate.duration_ms! - original.duration_ms),
    );
  if (!ranked.length)
    throw new SpotifyYouTubeLiveFailure(
      "No conservative candidate was found; an explicit human match decision is required",
    );
  return { ...ranked[0], qualifying: ranked.length };
}

export function retainedImportedPlaylist(
  detail: Schema["PlaylistDetail"],
  recording: string,
) {
  return {
    ...detail,
    entries: detail.entries.map((entry) =>
      entry.recording_id === recording ? { ...entry, available: false } : entry,
    ),
  };
}
