// @vitest-environment jsdom
import { createElement } from "react";
import { act, cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { NoticeProvider } from "./components/Notices";
import { audiobookPages } from "./fixtures/audiobooks";
import { audiobookProgressMedia } from "./fixtures/audiobookProgressMedia";
import type { components } from "./generated";
import { Playback } from "./playback";
import { PlaybackProvider, usePlayer } from "./player/PlaybackProvider";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("newer browser Pause retires Continue while its heard prefix waits for acknowledgement", async () => {
  vi.spyOn(console, "info").mockImplementation(() => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.spyOn(console, "debug").mockImplementation(() => {});
  const media = audiobookProgressMedia();
  const { detail: page } = audiobookPages();
  const chapter = page.detail.chapters[1];
  const anchor = {
    audiobook_id: page.detail.playlist.id,
    entry_id: chapter.entry_id,
    recording_id: chapter.recording_id!,
  };
  const source: components["schemas"]["Source"] = {
    id: "189b7475-24a0-4863-bb66-1492067a3fa3",
    recording_id: chapter.recording_id!,
    provider: "plugin",
    device_id: null,
    ready: true,
    can_prepare: false,
    label: "Owned fixture source",
  };
  const submitted: components["schemas"]["SyncBatch"][] = [];
  const acknowledgements: (() => void)[] = [];
  const requests: string[] = [];
  let reads = 0;
  vi.stubGlobal(
    "fetch",
    vi.fn<typeof globalThis.fetch>(async (input, options) => {
      const path = String(input);
      requests.push(path);
      expect(options?.credentials).toBe("same-origin");
      expect(new Headers(options?.headers).get("x-hito-account")).toBe(
        page.user.id,
      );
      if (path === "/api/v1/sync") {
        const batch: components["schemas"]["SyncBatch"] = JSON.parse(
          String(options?.body),
        );
        submitted.push(batch);
        expect(submitted.length).toBeLessThanOrEqual(2);
        return new Promise<Response>((resolve) =>
          acknowledgements.push(() =>
            resolve(
              Response.json({
                acknowledged: batch.operations.map(({ id }) => id),
              }),
            ),
          ),
        );
      }
      if (path.endsWith("/playback")) {
        reads++;
        throw new Error("Retired Continue must not request its audiobook");
      }
      expect(path).toBe(
        `/api/v1/recordings/${chapter.recording_id}/playback-sources`,
      );
      return Response.json([source]);
    }),
  );
  let player!: Playback;
  function Capture() {
    const current = usePlayer();
    if (!(current instanceof Playback))
      throw new Error("The owned browser player is required");
    player = current;
    return null;
  }
  const view = render(
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account: page.user.id,
        children: createElement(Capture),
      }),
    ),
  );
  const controller = new AbortController();
  try {
    await act(async () => {
      player.speed(1.5);
      await player.playQueue([
        {
          recording: { ...chapter, id: chapter.recording_id! },
          chapter: anchor,
        },
      ]);
    });
    const current = media.current();
    await act(async () => {
      media.ready(current);
      media.observePlaying(current);
    });
    await waitFor(() => expect(submitted).toHaveLength(1));
    await act(async () => media.advance(current, 2.75));
    const queue = player.queueSnapshot();
    const pageBefore = await player.queuePage(0, 10);
    const loaded = media.loaded.length;
    const played = media.played.length;
    const continuing = player.continueAudiobook(
      page.detail.playlist.id,
      controller.signal,
    );
    expect(reads).toBe(0);
    await act(async () => player.setPlaying(false));
    expect(await continuing).toBe(false);
    expect(current.paused).toBe(true);
    expect(player.snapshot().playing).toBe(false);
    expect(player.queueSnapshot()).toBe(queue);
    expect(player.snapshot().speed).toBe(1.5);
    expect(current.currentTime).toBe(2.75);
    expect(current.playbackRate).toBe(1.5);
    expect(media.loaded).toHaveLength(loaded);
    expect(media.played).toHaveLength(played);
    await act(async () => acknowledgements[0]());
    await waitFor(() => expect(submitted).toHaveLength(2));
    expect(submitted[1].operations.map(({ action }) => action)).toEqual([
      {
        kind: "progress_set",
        recording_id: chapter.recording_id,
        position_ms: 2750,
        speed: 1.5,
      },
      { kind: "audiobook_chapter_set", chapter: anchor },
    ]);
    await act(async () => acknowledgements[1]());
    expect(await player.queuePage(0, 10)).toEqual(pageBefore);
    expect(reads).toBe(0);
    expect(player.queueSnapshot()).toBe(queue);
    expect(media.current()).toBe(current);
    expect(current.getAttribute("src")).toBe(
      `/api/v1/sources/${source.id}/audio`,
    );
    expect(current.paused).toBe(true);
    expect(media.loaded).toHaveLength(loaded);
    expect(media.played).toHaveLength(played);
    expect(player.snapshot().error).toBe("");
    expect(
      requests.every(
        (path) =>
          path === "/api/v1/sync" ||
          path ===
            `/api/v1/recordings/${chapter.recording_id}/playback-sources`,
      ),
    ).toBe(true);
  } finally {
    acknowledgements.forEach((acknowledge) => acknowledge());
    controller.abort();
    view.unmount();
  }
});
