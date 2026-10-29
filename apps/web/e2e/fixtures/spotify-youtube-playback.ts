import { expect, type Page } from "@playwright/test";
import type { components } from "../../src/generated";
import { liveAudioState, liveControlBounds } from "./youtube-live";
import { openLibrarySection } from "./library-sections";

type Schema = components["schemas"];

export const audioPath =
  /^\/api\/v1\/sources\/[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}\/audio$/;

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

export async function verifySpotifyYouTubePlayback(
  page: Page,
  {
    receipt,
    recording,
    beforePlaylist,
    afterPlaylist,
    sources,
    original,
    origin,
    setPhase,
  }: {
    receipt: Schema["PlaylistImportReceipt"];
    recording: string;
    beforePlaylist: Schema["PlaylistDetail"];
    afterPlaylist: Schema["PlaylistDetail"];
    sources: Schema["Source"][];
    original: Schema["MetadataObservation"];
    origin: string;
    setPhase: (phase: 8 | 9 | 10) => void;
  },
) {
  setPhase(8);
  await openLibrarySection(page, "Playlists");
  await page
    .locator(`a[href="/playlists/${receipt.playlist_id}"]`)
    .first()
    .click();
  const playableTable = page.getByRole("table", {
    name: "Playlist entries",
    exact: true,
  });
  await expect(playableTable.getByRole("row")).toHaveCount(
    beforePlaylist.entries.length + 1,
  );
  await expect(playableTable.locator(".row-play:disabled")).toHaveCount(
    afterPlaylist.entries.filter(
      (entry) => entry.recording_id && entry.recording_id !== recording,
    ).length,
  );
  expect(
    await playableTable
      .locator("tbody > tr")
      .evaluateAll((rows) =>
        rows.every((row) => row.getBoundingClientRect().height > 0),
      ),
  ).toBe(true);
  const playableRow = playableTable
    .getByRole("row")
    .filter({ has: page.locator(`a[href="/recordings/${recording}"]`) })
    .first();
  const play = playableRow.getByRole("button", { name: /^Play .+/ });
  await expect(play).toBeEnabled();
  await play.click();
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
  const first = await liveAudioState(page);
  const source = new URL(first.source);
  expect(
    source.origin === origin &&
      !source.search &&
      !source.hash &&
      audioPath.test(source.pathname) &&
      sources.some(
        (item) => source.pathname === `/api/v1/sources/${item.id}/audio`,
      ),
  ).toBe(true);
  expect(
    Number.isFinite(first.duration) &&
      first.duration > 10 &&
      first.duration <= 600 &&
      Math.abs(first.duration * 1000 - original.duration_ms) <=
        Math.max(10_000, original.duration_ms * 0.08),
  ).toBe(true);
  await expect
    .poll(async () => (await liveAudioState(page)).time, {
      timeout: 10_000,
    })
    .toBeGreaterThan(first.time + 0.2);

  setPhase(9);
  const seek = page.getByRole("slider", { name: "Seek", exact: true });
  await expect(seek).toBeEnabled();
  const beforeSeek = await liveAudioState(page);
  await seek.focus();
  await seek.press("PageUp");
  await seek.press("PageUp");
  const target = Number(await seek.inputValue());
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

  setPhase(10);
  await openLibrarySection(page, "History");
  await expect
    .poll(async () => (await liveAudioState(page)).time, {
      timeout: 10_000,
    })
    .toBeGreaterThan(afterSeek.time + 0.2);
  const navigated = await liveAudioState(page);
  expect(
    navigated.source === first.source &&
      navigated.ready >= 2 &&
      navigated.error === 0 &&
      !navigated.paused,
  ).toBe(true);
  await liveControlBounds(page, seek, false);
  return { first, target, afterSeek, navigated };
}
