import type { Page, Route } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";

export type Scenario = {
  audio: "local" | "none" | "spotify" | "manual" | "guess" | "failing";
  lyrics?: Schema<"TimedLyrics"> | null;
  downloading?: boolean;
};

const TITLE = "Slow Tide";
const ARTIST = "Nadia Ferreira";
const ALBUM = "Casa Aberta";
const RELEASE = "6b5f7f3a-2a1c-4d0e-9d6f-3c1f2a9e8b11";
const MUSICBRAINZ = "b7d1c2e4-91f3-4a6a-9c58-0e2b3f8a1d77";
const SPOTIFY = "inst-spotify";
const JAMENDO = "inst-jamendo";
const LRCLIB = "inst-lrclib";

const at = (iso: string) => Math.floor(new Date(iso).getTime() / 1000);
const tag = (value: string): Schema<"ObservedTag"> => ({
  value,
  location: { kind: "format" },
});

export const lyricLines: Schema<"TimedLyricLine">[] = [
  [4, "A maré vem devagar"],
  [9, "e leva o que ficou na areia"],
  [14, "Deixa a luz do porto acesa"],
  [19, "que eu volto antes da chuva"],
  [31, "Não há pressa no rio"],
  [36, "só o barco a contar as horas"],
  [41, "e tu, do outro lado da água,"],
  [46, "a aprender a ficar"],
].map(([start, text]) => ({
  start_ms: Number(start) * 1000,
  end_ms: Number(start) * 1000 + 4000,
  text: String(text),
}));

export const syncedLyrics: Schema<"TimedLyrics"> = {
  id: "lyrics-slow-tide",
  rendition_id: "rendition-1",
  language: "pt",
  lines: lyricLines,
  attribution: {
    creators: [ARTIST],
    provider_label: "LRCLIB",
    content_url: "https://lrclib.example/slow-tide",
  },
};

function localObservation(): Schema<"MetadataObservation"> {
  return {
    id: "obs-local",
    title: "Slow Tide (Album Version)",
    artist: ARTIST,
    album: ALBUM,
    album_artist: ARTIST,
    disc_number: 1,
    track_number: 3,
    duration_ms: 252_000,
    origin: "file_inspection",
    provider: "local",
    provider_recording_id: null,
    provider_revision: null,
    release_id: RELEASE,
    source_id: "3a7d2c10-6f4e-4b9a-8c1d-2e3f4a5b6c7d",
    plugin_installation_id: null,
    inspection_version: 1,
    observed_at: at("2026-11-06T11:22:00Z"),
    attribution: null,
    tags: {
      title: tag("Slow Tide (Album Version)"),
      artist: tag(ARTIST),
      album: tag(ALBUM),
      album_artist: tag(ARTIST),
      disc: tag("1"),
      track: tag("3/11"),
      track_total: null,
      date: tag("2021-05-04"),
      label: tag("Maré Discos"),
      audio: { format: "FLAC", bit_depth: 16, sample_rate_hz: 44_100 },
      genre: null,
      isrc: tag("PTLD02100034"),
      musicbrainz_album_id: null,
      musicbrainz_artist_tags: null,
      musicbrainz_recording_id: tag(MUSICBRAINZ),
      musicbrainz_release_group_id: null,
      musicbrainz_release_track_id: null,
    },
  };
}

function spotifyObservation(): Schema<"MetadataObservation"> {
  return {
    id: "obs-spotify",
    title: TITLE,
    artist: ARTIST,
    album: ALBUM,
    album_artist: ARTIST,
    disc_number: 1,
    track_number: 3,
    duration_ms: 252_000,
    origin: "provider_response",
    provider: "Spotify",
    provider_recording_id: "spotify:track:slow-tide",
    provider_revision: "rev-3",
    release_id: RELEASE,
    source_id: "9c1e5b22-7a3d-4f6e-b8a0-1d2c3b4a5f6e",
    plugin_installation_id: SPOTIFY,
    inspection_version: null,
    observed_at: at("2026-10-28T20:12:00Z"),
    attribution: null,
    tags: null,
  };
}

function catalogObservation(): Schema<"MetadataObservation"> {
  return {
    ...spotifyObservation(),
    id: "obs-musicbrainz",
    provider: "MusicBrainz",
    provider_recording_id: MUSICBRAINZ,
    provider_revision: null,
    source_id: null,
    plugin_installation_id: null,
  };
}

export function fidelityDetail(
  recording: string,
  scenario: Scenario,
): Schema<"RecordingMetadata"> {
  const local = localObservation();
  const selected =
    scenario.audio === "local"
      ? local
      : scenario.audio === "none"
        ? catalogObservation()
        : spotifyObservation();
  const items =
    scenario.audio === "local" ? [local, spotifyObservation()] : [selected];
  return {
    recording_id: recording,
    title: {
      value: TITLE,
      inherited: "Slow Tide (Album Version)",
      personal: TITLE,
      operation_id: "op-title",
    },
    artist: {
      value: ARTIST,
      inherited: ARTIST,
      personal: null,
      operation_id: null,
    },
    credited_artists: [
      { name: ARTIST, artist_id: "artist-nadia", join: null, role: "main" },
    ],
    musicbrainz_recording_id: MUSICBRAINZ,
    selected_observation: selected,
    observations: { items, next_before: null },
    audio_format:
      scenario.audio === "local"
        ? {
            format: "FLAC",
            bit_depth: 16,
            sample_rate_hz: 44_100,
            size_bytes: 31_400_000,
          }
        : null,
    release_facts: { year: 2021, track_total: 11, label: "Maré Discos" },
    last_played: {
      played_at: Math.floor(Date.now() / 1000) - 2 * 86_400,
      position_ms: 97_000,
    },
    appears_in: [
      { id: "pl-late-night", kind: "local", name: "Late night" },
      { id: "pl-road-trip", kind: "local", name: "Road trip" },
    ],
  };
}

export function fidelityCopies(recording: string): Schema<"RecordingCopies"> {
  return {
    canonical_id: recording,
    copies: [
      {
        recording_id: recording,
        title: TITLE,
        artist: ARTIST,
        album: ALBUM,
        available: true,
        canonical: true,
        proof: null,
        sources: [],
      },
      {
        recording_id: "2f1e6c7a-5b3d-4e8f-9a0b-1c2d3e4f5a6b",
        title: TITLE,
        artist: ARTIST,
        album: "Casa Aberta (Deluxe)",
        available: true,
        canonical: false,
        proof: "isrc",
        sources: [],
      },
    ],
  };
}

export const fidelityPlaylists: Schema<"Playlist">[] = [
  { id: "pl-late-night", kind: "local", name: "Late night" },
  { id: "pl-road-trip", kind: "local", name: "Road trip" },
  { id: "pl-treino", kind: "local", name: "Treino de domingo" },
];

function source(
  id: string,
  provider: string,
  label: string,
  ready = true,
): Schema<"Source"> {
  return {
    id,
    recording_id: "",
    provider,
    label,
    device_id: null,
    ready,
    can_prepare: false,
    match_kind: "verified",
  };
}

export function fidelityChoice(
  recording: string,
  scenario: Scenario,
): Schema<"SourceChoice"> {
  const local = {
    ...source("3a7d2c10-6f4e-4b9a-8c1d-2e3f4a5b6c7d", "local", "Local file"),
    recording_id: recording,
  };
  const spotify = {
    ...source("9c1e5b22-7a3d-4f6e-b8a0-1d2c3b4a5f6e", "Spotify", "Spotify"),
    recording_id: recording,
  };
  const entry = (item: Schema<"Source">, selected: boolean) => ({
    source: item,
    selected,
    selectable: true,
  });
  if (scenario.audio === "local")
    return {
      recording_id: recording,
      revision: 3,
      mode: "automatic",
      sources: [entry(local, true)],
    };
  if (scenario.audio === "spotify")
    return {
      recording_id: recording,
      revision: 3,
      mode: "automatic",
      sources: [entry(spotify, true)],
    };
  if (scenario.audio === "guess")
    return {
      recording_id: recording,
      revision: 3,
      mode: "automatic",
      sources: [
        entry(
          {
            ...spotify,
            match_kind: "best_guess",
            provider_label: "Jamendo",
          },
          true,
        ),
      ],
    };
  if (scenario.audio === "manual")
    return {
      recording_id: recording,
      revision: 4,
      mode: "manual",
      sources: [entry(local, false), entry(spotify, true)],
    };
  return {
    recording_id: recording,
    revision: 1,
    mode: "automatic",
    sources: [],
  };
}

const manifest = (
  id: string,
  name: string,
  capabilities: Schema<"Capability">[],
): Schema<"PluginInstallation"> => ({
  id,
  service_id: `svc-${id}`,
  revision: 1,
  checked_at: at("2026-11-06T10:00:00Z"),
  credential_set: true,
  manifest: { id: `${id}-manifest`, name, protocol_version: 1, capabilities },
});

export const fidelityPlugins: Schema<"PluginSettings"> = {
  credential_storage_ready: true,
  installations: [
    manifest(SPOTIFY, "Spotify", ["search", "sources", "catalog"]),
    manifest(JAMENDO, "Jamendo", ["search", "sources"]),
    manifest(LRCLIB, "LRCLIB", ["lyrics"]),
  ],
  services: [
    { id: `svc-${SPOTIFY}`, name: "Spotify", base_url: "", revision: 1 },
    { id: `svc-${JAMENDO}`, name: "Jamendo", base_url: "", revision: 1 },
    { id: `svc-${LRCLIB}`, name: "LRCLIB", base_url: "", revision: 1 },
  ],
};

export const fidelityCandidates: Schema<"ProviderRecording">[] = [
  {
    id: "spotify:track:slow-tide",
    title: TITLE,
    artist: ARTIST,
    duration_ms: 252_000,
    release: { id: "spotify:album:casa-aberta", title: ALBUM, artist: ARTIST },
  },
  {
    id: "jamendo:track:slow-tide-live",
    title: "Slow Tide (Ao vivo no CCB)",
    artist: ARTIST,
    duration_ms: 301_000,
    release: null,
  },
  {
    id: "spotify:track:slow-tide-rework",
    title: "Slow Tide (Rework)",
    artist: "Nadia Ferreira, Tó Pinto",
    duration_ms: 380_000,
    release: null,
  },
];

const cover = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 320 320">
<defs>
<linearGradient id="b" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#17243a"/><stop offset="1" stop-color="#0e1826"/></linearGradient>
<radialGradient id="c" cx="0.2" cy="0.15" r="0.9"><stop offset="0" stop-color="#2f6f8f"/><stop offset="0.55" stop-color="#2f6f8f" stop-opacity="0"/></radialGradient>
<radialGradient id="d" cx="0.85" cy="0.8" r="0.9"><stop offset="0" stop-color="#c8743b"/><stop offset="0.55" stop-color="#c8743b" stop-opacity="0"/></radialGradient>
<pattern id="l" width="11" height="11" patternUnits="userSpaceOnUse"><rect width="11" height="1" fill="rgba(255,255,255,0.06)"/></pattern>
</defs>
<rect width="320" height="320" fill="url(#b)"/><rect width="320" height="320" fill="url(#c)"/><rect width="320" height="320" fill="url(#d)"/><rect width="320" height="320" fill="url(#l)"/>
<text x="26" y="296" font-family="ui-monospace, monospace" font-size="20" letter-spacing="3.2" fill="rgba(255,255,255,0.75)">CASA ABERTA</text>
</svg>`;

function inertiaProps(
  html: string,
  replace: (props: Record<string, unknown>) => void,
) {
  const match = html.match(
    /<script data-page="app" type="application\/json">(.*?)<\/script>/s,
  );
  if (!match) throw new Error("The page did not embed its Inertia props");
  const page = JSON.parse(match[1]) as { props: Record<string, unknown> };
  replace(page.props);
  const encoded = JSON.stringify(page).replace(/</g, "\\u003c");
  return html.replace(
    match[0],
    `<script data-page="app" type="application/json">${encoded}</script>`,
  );
}

export async function stubTrackPage(
  page: Page,
  recording: string,
  scenario: Scenario,
) {
  const base = `/api/v1/recordings/${recording}`;
  const detail = fidelityDetail(recording, scenario);
  const copies = fidelityCopies(recording);
  const swap = (props: Record<string, unknown>) => {
    props.detail = detail;
    props.copies = copies;
    props.liked = true;
    props.playlists = fidelityPlaylists;
  };
  await page.route(
    (url) => url.pathname === `/recordings/${recording}`,
    async (route: Route) => {
      const response = await route.fetch();
      const type = response.headers()["content-type"] ?? "";
      if (type.includes("application/json")) {
        const json = await response.json();
        swap(json.props);
        return route.fulfill({ response, json });
      }
      const html = await response.text();
      return route.fulfill({
        response,
        body: inertiaProps(html, swap),
        contentType: "text/html",
      });
    },
  );
  await page.route(`**${base}/artwork`, (route) =>
    route.fulfill({ body: cover, contentType: "image/svg+xml" }),
  );
  await page.route(`**/api/v1/releases/${RELEASE}/artwork`, (route) =>
    route.fulfill({ body: cover, contentType: "image/svg+xml" }),
  );
  await page.route(`**${base}/source-choice`, (route) =>
    scenario.audio === "failing"
      ? route.fulfill({ status: 503, json: { error: "Service unavailable" } })
      : route.fulfill({ json: fidelityChoice(recording, scenario) }),
  );
  await page.route(`**${base}/download`, (route) =>
    scenario.downloading
      ? route.fulfill({
          json: {
            intent_id: "7c2b9e4d-1a5f-4c3e-8d6b-0f9e8d7c6b5a",
            recording_id: recording,
            source_id: "9c1e5b22-7a3d-4f6e-b8a0-1d2c3b4a5f6e",
            host_device_id: "server",
            choice_revision: 3,
            matches_source_choice: true,
            status: "running",
            bytes_received: 19_400_000,
            total_bytes: 31_400_000,
            error: null,
          } satisfies Schema<"DownloadStatus">,
        })
      : route.fulfill({ status: 404, json: { error: "Not found" } }),
  );
  await page.route("**/api/v1/plugins", (route) =>
    route.request().method() === "GET"
      ? route.fulfill({ json: fidelityPlugins })
      : route.continue(),
  );
  const identity = {
    selectable: true,
    selector: { kind: "observation", observation_id: "obs-lrclib" },
    installation_id: LRCLIB,
    installation_revision: 1,
    label: "LRCLIB",
    provider_recording: { id: "lrclib:slow-tide", revision: null },
  } satisfies Schema<"RecordingLyricIdentity">;
  await page.route(`**${base}/lyrics/identities`, (route) =>
    route.fulfill({
      json: {
        recording_id: recording,
        items: [identity],
      } satisfies Schema<"RecordingLyricIdentities">,
    }),
  );
  await page.route(`**${base}/lyrics/services`, (route) =>
    route.fulfill({
      json: {
        recording_id: recording,
        items: [
          {
            installation_id: LRCLIB,
            installation_revision: 1,
            label: "LRCLIB",
          },
        ],
      } satisfies Schema<"RecordingLyricServices">,
    }),
  );
  await page.route(`**${base}/lyrics/identities/read`, (route) =>
    route.fulfill({
      json: {
        status: "ready_v1",
        recording_id: recording,
        selector: identity.selector,
        provider_recording: identity.provider_recording,
        document_digest: "0".repeat(64),
        document: {
          recording: identity.provider_recording,
          lyrics: scenario.lyrics ?? null,
        },
      } satisfies Schema<"SelectedLyricsLookup">,
    }),
  );
  await page.route("**/api/v1/plugins/*/search", (route) =>
    route.fulfill({
      json: { items: fidelityCandidates, next_cursor: null },
    }),
  );
  await page.route(`**${base}/sources/review`, (route) =>
    route.fulfill({
      json: {
        candidate: fidelityCandidates[0],
        review_stamp: "stamp-1",
      } satisfies Schema<"ProviderSourceReview">,
    }),
  );
}
