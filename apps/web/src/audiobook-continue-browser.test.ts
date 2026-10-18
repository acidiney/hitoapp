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

type Schema = components["schemas"];
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("browser Continue reads fresh exact saved playback after its fixed heard prefix while later progress stays pending", async () => {
  vi.spyOn(console, "info").mockImplementation(() => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.spyOn(console, "debug").mockImplementation(() => {});
  const media = audiobookProgressMedia();
  const { detail: page } = audiobookPages();
  const [first, middle, last] = page.detail.chapters;
  const pending = {
    ...middle,
    entry_id: "10895642-d265-48c7-aadb-93d6f754158c",
    recording_id: "eb551623-a1da-4073-bc64-dd0156159b27",
    position: 25,
    available: false,
  };
  const chapter = (entry: typeof first): Schema["AudiobookChapterAnchor"] => ({
    audiobook_id: page.detail.playlist.id,
    entry_id: entry.entry_id,
    recording_id: entry.recording_id!,
  });
  const fresh: Schema["AudiobookPlayback"] = {
    audiobook_id: page.detail.playlist.id,
    chapter: chapter(last),
    progress: {
      recording_id: last.recording_id!,
      position_ms: 35125,
      speed: 0.75,
    },
    chapters: [first, middle, pending, last],
  };
  const sources: Schema["Source"][] = [first, middle].map((entry, index) => ({
    id: index
      ? "189b7475-24a0-4863-bb66-1492067a3fa3"
      : "47f373bb-4296-4e89-bd3c-5b3e73833ac5",
    recording_id: entry.recording_id!,
    provider: "plugin",
    device_id: null,
    ready: true,
    can_prepare: false,
    label: "Owned fixture source",
  }));
  const requests: string[] = [];
  const submitted: Schema["SyncBatch"][] = [];
  const acknowledgements: (() => void)[] = [];
  let reads = 0;
  let readSignal: AbortSignal | undefined;
  let releaseRead!: () => void;
  const heldRead = new Promise<void>((resolve) => {
    releaseRead = resolve;
  });
  const bookPath = `/api/v1/audiobooks/${page.detail.playlist.id}/playback`;
  vi.stubGlobal(
    "fetch",
    vi.fn<typeof globalThis.fetch>(async (input, options) => {
      const path = String(input);
      requests.push(path);
      expect(options?.credentials).toBe("same-origin");
      expect(new Headers(options?.headers).get("x-hito-account")).toBe(
        page.user.id,
      );
      expect(new Headers(options?.headers).get("x-hito-request")).toBe("1");
      expect(options?.signal).toBeInstanceOf(AbortSignal);
      if (path === "/api/v1/sync") {
        expect(options?.method).toBe("POST");
        const batch: Schema["SyncBatch"] = JSON.parse(String(options?.body));
        submitted.push(batch);
        expect(submitted.length).toBeLessThanOrEqual(4);
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
      expect(options?.method).toBe("GET");
      expect(options?.body).toBeUndefined();
      if (path === bookPath) {
        reads++;
        readSignal = options?.signal as AbortSignal;
        await heldRead;
        return Response.json(fresh);
      }
      const source = sources.find(
        (source) =>
          path === `/api/v1/recordings/${source.recording_id}/sources`,
      );
      if (!source)
        throw new Error("Unexpected owned browser continuation request");
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
  const checkpoint = (
    index: number,
    position: number,
    speed: number,
    entry: typeof first,
  ) => {
    const batch = submitted[index];
    expect(batch.operations.map(({ action }) => action)).toEqual([
      {
        kind: "progress_set",
        recording_id: entry.recording_id,
        position_ms: position,
        speed,
      },
      { kind: "audiobook_chapter_set", chapter: chapter(entry) },
    ]);
  };
  try {
    await act(async () => {
      player.speed(1.5);
      await player.playQueue([
        {
          recording: { ...middle, id: middle.recording_id! },
          chapter: chapter(middle),
        },
      ]);
    });
    const current = media.current();
    await act(async () => {
      media.ready(current);
      media.observePlaying(current);
    });
    await waitFor(() => expect(submitted).toHaveLength(1));
    checkpoint(0, 0, 1.5, middle);
    const before = player.queueSnapshot();
    const oldQueue = await player.queuePage(0, 10);
    await act(async () => {
      media.advance(current, 2.75);
    });
    expect(
      current.seeking,
      "The heard clock advance must not be an unfinished seek",
    ).toBe(false);
    const continued = player.continueAudiobook(
      page.detail.playlist.id,
      controller.signal,
    );
    expect(reads).toBe(0);
    await act(async () => {
      media.advance(current, 15.25);
    });
    expect(current.seeking).toBe(false);
    expect(submitted).toHaveLength(1);
    expect(player.queueSnapshot()).toBe(before);
    expect(player.snapshot().speed).toBe(1.5);
    expect(current.paused).toBe(false);
    await act(async () => acknowledgements[0]());
    await waitFor(() => expect(submitted).toHaveLength(2));
    checkpoint(1, 2750, 1.5, middle);
    expect(reads).toBe(0);
    await act(async () => acknowledgements[1]());
    await waitFor(() => expect(submitted).toHaveLength(3));
    checkpoint(2, 15250, 1.5, middle);
    await waitFor(() => expect(reads).toBe(1));
    expect(readSignal?.aborted).toBe(false);
    expect(media.current()).toBe(current);
    expect(current.getAttribute("src")).toBe(
      `/api/v1/sources/${sources[1].id}/audio`,
    );
    expect(current.currentTime).toBe(15.25);
    expect(current.paused).toBe(false);
    expect(current.playbackRate).toBe(1.5);
    expect(player.queueSnapshot()).toBe(before);
    expect(await player.queuePage(0, 10)).toEqual(oldQueue);
    expect(player.snapshot().speed).toBe(1.5);
    await act(async () => {
      releaseRead();
      expect(await continued).toBe(true);
    });
    const queue = await player.queuePage(0, 10);
    expect(queue.entries.map(({ chapter }) => chapter?.entry_id)).toEqual(
      fresh.chapters.map(({ entry_id }) => entry_id),
    );
    expect(queue.entries[2].recording.available).toBe(false);
    expect(queue.entries[2].recording).toMatchObject({ liked: pending.liked });
    expect(queue.entries[0].recording.id).toBe(queue.entries[3].recording.id);
    expect(queue.entries[0].id).not.toBe(queue.entries[3].id);
    expect(queue.entries[3].chapter).toEqual(fresh.chapter);
    expect(queue.entries[3].id).not.toBe(fresh.chapter.entry_id);
    expect(player.queueSnapshot()).toMatchObject({
      total: 4,
      currentIndex: 3,
      currentEntry: queue.entries[3].id,
    });
    expect(player.queueSnapshot().version).toBe(before.version + 1);
    expect(player.snapshot().speed).toBe(0.75);
    expect(submitted).toHaveLength(3);
    await act(async () => media.ready(media.current()));
    expect(media.current().currentTime).toBe(35.125);
    expect(media.current().playbackRate).toBe(0.75);
    expect(media.current().seeking).toBe(true);
    expect(submitted).toHaveLength(3);
    await act(async () => media.completeSeek(media.current()));
    await act(async () => acknowledgements[2]());
    await waitFor(() => expect(submitted).toHaveLength(4));
    checkpoint(3, 35125, 0.75, last);
    expect(new Set(submitted.map(({ device_id }) => device_id)).size).toBe(1);
    expect(
      new Set(
        submitted.flatMap(({ operations }) => operations.map(({ id }) => id)),
      ).size,
    ).toBe(8);
    await act(async () => acknowledgements[3]());
    expect(reads).toBe(1);
    expect(
      requests.every(
        (path) =>
          path === "/api/v1/sync" ||
          path === bookPath ||
          sources.some(
            (source) =>
              path === `/api/v1/recordings/${source.recording_id}/sources`,
          ),
      ),
    ).toBe(true);
    expect(player.snapshot().error).toBe("");
  } finally {
    releaseRead();
    acknowledgements.forEach((acknowledge) => acknowledge());
    controller.abort();
    view.unmount();
  }
});
