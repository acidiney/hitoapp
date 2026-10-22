import { createElement } from "react";
import { act, render } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import { NoticeProvider } from "./shared/ui/Notices";
import { audiobookPages } from "./fixtures/audiobooks";
import { audiobookProgressMedia } from "./fixtures/audiobookProgressMedia";
import type { components } from "./generated";
import { Playback } from "./playback";
import { PlaybackProvider, usePlayer } from "./player/PlaybackProvider";

type Schema = components["schemas"];
beforeEach(() => {
  vi.spyOn(console, "info").mockImplementation(() => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.spyOn(console, "debug").mockImplementation(() => {});
});
test("a stalled Continue body expires visibly without changing current playback and only an explicit retry can commit", async () => {
  const f = await playingBook();
  const held = heldPlayback(f.fresh);
  f.service.read = async () => held.response;
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  const controller = new AbortController();
  try {
    let continuing!: Promise<boolean>;
    await act(async () => {
      continuing = f.player.continueAudiobook(f.book, controller.signal);
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(f.service.reads).toHaveLength(1);
    expect(held.response.bodyUsed).toBe(true);
    await act(async () => vi.advanceTimersByTimeAsync(9999));
    expect(f.service.reads[0].aborted).toBe(false);
    expect(f.player.snapshot().error).toBe("");
    expect(f.current.paused).toBe(false);
    await act(async () => vi.advanceTimersByTimeAsync(1));
    expect(await continuing).toBe(false);
    expect(f.service.reads[0].aborted).toBe(true);
    expect(f.player.snapshot().error).toMatch(/could not continue|timed out/i);
    expect(f.player.queueSnapshot()).toBe(f.queue);
    expect(await f.player.queuePage(0, 10)).toEqual(f.entries);
    expect(f.player.snapshot().speed).toBe(1.5);
    expect(f.current.currentTime).toBe(2.75);
    expect(f.current.playbackRate).toBe(1.5);
    expect(f.current.paused).toBe(false);
    await act(async () => {
      held.release();
      await vi.advanceTimersByTimeAsync(60000);
    });
    expect(f.service.reads).toHaveLength(1);
    expect(f.player.queueSnapshot()).toBe(f.queue);
    expect(f.media.played).toHaveLength(1);
    f.service.read = async () => Response.json(f.fresh);
    await act(async () =>
      expect(await f.player.continueAudiobook(f.book, controller.signal)).toBe(
        true,
      ),
    );
    const queue = await f.player.queuePage(0, 10);
    expect(f.service.reads).toHaveLength(2);
    expect(f.player.queueSnapshot()).toMatchObject({
      total: 3,
      currentIndex: 2,
      currentEntry: queue.entries[2].id,
    });
    expect(queue.entries[2].chapter).toEqual(f.fresh.chapter);
    expect(f.player.snapshot().speed).toBe(0.75);
    expect(f.player.positionSnapshot().position).toBe(35.125);
    expect(f.player.snapshot().error).toBe("");
  } finally {
    controller.abort();
    held.release();
    f.view.unmount();
  }
});

test("a retired browser account cannot install its late saved chapter into the replacement account", async () => {
  const f = await playingBook();
  const held = heldPlayback(f.fresh);
  f.service.read = async () => held.response;
  const controller = new AbortController();
  try {
    let continuing!: Promise<boolean>;
    await act(async () => {
      continuing = f.player.continueAudiobook(f.book, controller.signal);
    });
    expect(f.service.reads).toHaveLength(1);
    expect(held.response.bodyUsed).toBe(true);
    await act(async () => f.view.rerender(f.tree("replacement-account")));
    expect(await continuing).toBe(false);
    expect(f.service.reads[0].aborted).toBe(true);
    const replacement = f.currentPlayer();
    expect(replacement).not.toBe(f.player);
    expect(replacement.queueSnapshot().total).toBe(0);
    expect(replacement.snapshot().current).toBeNull();
    expect(replacement.snapshot().speed).toBe(1);
    expect(f.current.isConnected).toBe(false);
    expect(f.current.paused).toBe(true);
    const requests = f.service.requests.length;
    const writes = f.service.writes.length;
    await act(async () => held.release());
    expect(f.service.requests).toHaveLength(requests);
    expect(f.service.writes).toHaveLength(writes);
    expect(f.service.requests.every(({ account }) => account === "alice")).toBe(
      true,
    );
    expect(replacement.queueSnapshot().total).toBe(0);
    expect(replacement.snapshot().current).toBeNull();
    expect(f.player.queueSnapshot().total).toBe(0);
    expect(f.player.snapshot().current).toBeNull();
    expect(document.querySelectorAll("audio")).toHaveLength(1);
    expect(f.media.current().getAttribute("src")).toBeNull();
    expect(f.media.played).toHaveLength(1);
  } finally {
    controller.abort();
    held.release();
    f.view.unmount();
  }
});

async function playingBook() {
  const media = audiobookProgressMedia();
  const { detail: page } = audiobookPages();
  const chapters = page.detail.chapters.slice(0, 3);
  const anchor = (index: number) => ({
    audiobook_id: page.detail.playlist.id,
    entry_id: chapters[index].entry_id,
    recording_id: chapters[index].recording_id!,
  });
  const fresh: Schema["AudiobookPlayback"] = {
    audiobook_id: page.detail.playlist.id,
    chapter: anchor(2),
    progress: {
      recording_id: chapters[2].recording_id!,
      position_ms: 35125,
      speed: 0.75,
    },
    chapters,
  };
  const service = {
    requests: [] as { path: string; account: string | null }[],
    reads: [] as AbortSignal[],
    writes: [] as Schema["SyncBatch"][],
    read: async (): Promise<Response> => Response.json(fresh),
  };
  vi.stubGlobal(
    "fetch",
    vi.fn<typeof globalThis.fetch>(async (input, options) => {
      const path = String(input);
      service.requests.push({
        path,
        account: new Headers(options?.headers).get("x-hito-account"),
      });
      expect(options?.credentials).toBe("same-origin");
      expect(options?.signal).toBeInstanceOf(AbortSignal);
      if (path === "/api/v1/sync") {
        const batch: Schema["SyncBatch"] = JSON.parse(String(options?.body));
        service.writes.push(batch);
        return Response.json({
          acknowledged: batch.operations.map(({ id }) => id),
        });
      }
      expect(options?.method).toBe("GET");
      if (path === `/api/v1/audiobooks/${page.detail.playlist.id}/playback`) {
        service.reads.push(options?.signal as AbortSignal);
        return service.read();
      }
      const index = chapters.findIndex(
        (entry) =>
          path === `/api/v1/recordings/${entry.recording_id}/playback-sources`,
      );
      if (index < 0) throw new Error("Unexpected owned continuation request");
      return Response.json([
        {
          id:
            index === 1
              ? "189b7475-24a0-4863-bb66-1492067a3fa3"
              : "47f373bb-4296-4e89-bd3c-5b3e73833ac5",
          recording_id: chapters[index].recording_id!,
          provider: "plugin",
          device_id: null,
          ready: true,
          can_prepare: false,
          label: "Owned fixture source",
        } satisfies Schema["Source"],
      ]);
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
  const tree = (account: string) =>
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account,
        children: createElement(Capture),
      }),
    );
  const view = render(tree(page.user.id));
  await act(async () => {
    player.speed(1.5);
    await player.playQueue([
      {
        recording: { ...chapters[1], id: chapters[1].recording_id! },
        chapter: anchor(1),
      },
    ]);
  });
  const current = media.current();
  await act(async () => {
    media.ready(current);
    media.observePlaying(current);
  });
  expect(service.writes[0].operations.map(({ action }) => action)).toEqual([
    {
      kind: "progress_set",
      recording_id: chapters[1].recording_id,
      position_ms: 0,
      speed: 1.5,
    },
    { kind: "audiobook_chapter_set", chapter: anchor(1) },
  ]);
  await act(async () => media.advance(current, 2.75));
  return {
    player,
    currentPlayer: () => player,
    media,
    current,
    fresh,
    service,
    view,
    tree,
    book: page.detail.playlist.id,
    queue: player.queueSnapshot(),
    entries: await player.queuePage(0, 10),
  };
}

function heldPlayback(value: Schema["AudiobookPlayback"]) {
  const bytes = new TextEncoder().encode(JSON.stringify(value));
  let stream!: ReadableStreamDefaultController<Uint8Array>;
  let released = false;
  const response = new Response(
    new ReadableStream<Uint8Array>({
      start(controller) {
        stream = controller;
        stream.enqueue(bytes.slice(0, 1));
      },
      cancel() {
        released = true;
      },
    }),
    { headers: { "content-type": "application/json" } },
  );
  return {
    response,
    release() {
      if (released) return;
      released = true;
      stream.enqueue(bytes.slice(1));
      stream.close();
    },
  };
}
