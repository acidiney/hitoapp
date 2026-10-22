import { createElement } from "react";
import { act, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { NoticeBanner, NoticeProvider } from "./shared/ui/Notices";
import { audiobookPages } from "./fixtures/audiobooks";
import { audiobookProgressMedia } from "./fixtures/audiobookProgressMedia";
import { PlaybackProvider, usePlayer } from "./player/PlaybackProvider";
import type { Player } from "./player/model";
import type { Source, SyncBatch } from "./shared/types";

test("256 pending checkpoints pause at capacity and preserve the heard chapter before another queue can replace it", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  vi.spyOn(console, "info").mockImplementation(() => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.spyOn(console, "debug").mockImplementation(() => {});
  const savedClient = localStorage.getItem("hito-device");
  const client = "584f9d08-5ce1-417d-b7a6-8c09c3ba7b47";
  localStorage.setItem("hito-device", client);
  const media = audiobookProgressMedia();
  const { detail: page } = audiobookPages();
  const [first, second] = page.detail.chapters;
  const selection = (chapter: typeof first) => ({
    recording: { ...chapter, id: chapter.recording_id! },
    chapter: {
      audiobook_id: page.detail.playlist.id,
      entry_id: chapter.entry_id,
      recording_id: chapter.recording_id!,
    },
  });
  const source: Source = {
    id: "76769d98-1b6b-4ec8-8ff7-478c04d7bb23",
    recording_id: first.recording_id!,
    provider: "plugin",
    device_id: null,
    ready: true,
    label: "Owned audio service",
    can_prepare: false,
  };
  const submitted: { raw: string; batch: SyncBatch }[] = [];
  const requests: string[] = [];
  const pending = new Set<Promise<Response>>();
  let releaseFirst: (() => void) | undefined;
  let drained!: () => void;
  const allSubmitted = new Promise<void>((resolve) => {
    drained = resolve;
  });
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, options?: RequestInit) => {
      expect(options?.credentials).toBe("same-origin");
      expect(new Headers(options?.headers).get("x-hito-account")).toBe(
        page.user.id,
      );
      expect(new Headers(options?.headers).get("x-hito-request")).toBe("1");
      const signal = options?.signal as AbortSignal;
      expect(signal).toBeInstanceOf(AbortSignal);
      requests.push(`${options?.method} ${String(input)}`);
      if (
        input === `/api/v1/recordings/${first.recording_id}/playback-sources` &&
        options?.method === "GET"
      )
        return Response.json([source]);
      if (input === "/api/v1/sync" && options?.method === "POST") {
        const raw = String(options.body);
        const batch = JSON.parse(raw) as SyncBatch;
        submitted.push({ raw, batch });
        const receipt = () =>
          Response.json({ acknowledged: batch.operations.map(({ id }) => id) });
        if (submitted.length === 257) drained();
        if (submitted.length !== 1) return receipt();
        let removeAbort!: () => void;
        const response = new Promise<Response>((resolve, reject) => {
          const abort = () =>
            reject(new DOMException("The request was aborted", "AbortError"));
          signal.addEventListener("abort", abort, { once: true });
          removeAbort = () => signal.removeEventListener("abort", abort);
          releaseFirst = () => resolve(receipt());
          if (signal.aborted) abort();
        });
        pending.add(response);
        try {
          return await response;
        } finally {
          removeAbort();
          pending.delete(response);
        }
      }
      throw new Error("Capacity recovery requested an unowned operation");
    }),
  );
  let player!: Player;
  function Capture() {
    player = usePlayer();
    return createElement(NoticeBanner);
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
  try {
    await act(async () => player.playQueue([selection(first)]));
    const audio = media.current();
    const pause = vi.fn();
    audio.addEventListener("pause", pause);
    await act(async () => {
      media.ready(audio);
      media.observePlaying(audio);
    });
    const positions = [0];
    await act(async () => {
      for (let index = 0; index < 255; index++) {
        const position = index % 2 === 0 ? 20 : 0;
        positions.push(position);
        player.seek(position);
        expect(audio.seeking).toBe(true);
        media.completeSeek(audio);
      }
    });
    expect(positions).toHaveLength(256);
    expect(submitted).toHaveLength(1);
    expect(player.snapshot().playing).toBe(true);
    expect(screen.queryByRole("status")).toBeNull();
    expect(pause).not.toHaveBeenCalled();
    const queue = await player.queuePage(0, 10);
    const queueView = player.queueSnapshot();
    const loaded = [...media.loaded];
    const played = [...media.played];

    await act(async () => {
      player.speed(1.75);
      player.seek(0);
      expect(audio.seeking).toBe(true);
      media.completeSeek(audio);
    });
    const message =
      "Listening progress is waiting to be saved. Playback paused.";
    expect(pause).toHaveBeenCalledTimes(1);
    expect(audio.paused).toBe(true);
    expect(player.snapshot().playing).toBe(false);
    expect(player.snapshot().error).toBe(message);
    expect(screen.getByRole("status").textContent).toBe(message);
    expect(submitted).toHaveLength(1);
    await act(async () => {
      await expect(player.playQueue([selection(second)])).rejects.toThrow(
        message,
      );
    });
    expect(player.queueSnapshot()).toEqual(queueView);
    expect(await player.queuePage(0, 10)).toEqual(queue);
    expect(player.snapshot().current?.id).toBe(first.recording_id);
    expect(player.positionSnapshot().position).toBe(0);
    expect(player.snapshot().speed).toBe(1.75);
    expect(media.current()).toBe(audio);
    expect(media.loaded).toEqual(loaded);
    expect(media.played).toEqual(played);
    expect(pause).toHaveBeenCalledTimes(1);
    expect(requests).toEqual([
      `GET /api/v1/recordings/${first.recording_id}/playback-sources`,
      "POST /api/v1/sync",
    ]);

    await act(async () => {
      releaseFirst!();
      await allSubmitted;
    });
    expect(submitted).toHaveLength(257);
    expect(
      submitted.map(({ batch }) =>
        batch.operations.map(({ action }) => action),
      ),
    ).toEqual(
      [
        ...positions.map((position) => ({ position, speed: 1 })),
        { position: 0, speed: 1.75 },
      ].map(({ position, speed }) => [
        {
          kind: "progress_set",
          recording_id: first.recording_id,
          position_ms: position * 1000,
          speed,
        },
        { kind: "audiobook_chapter_set", chapter: selection(first).chapter },
      ]),
    );
    expect(
      submitted.every(
        ({ raw, batch }) =>
          raw === JSON.stringify(batch) && batch.device_id === client,
      ),
    ).toBe(true);
    const ids = submitted.flatMap(({ batch }) =>
      batch.operations.map(({ id }) => id),
    );
    expect(new Set(ids).size).toBe(514);
    expect(player.queueSnapshot()).toEqual(queueView);
    expect(await player.queuePage(0, 10)).toEqual(queue);
    expect(media.current()).toBe(audio);
    expect(audio.paused).toBe(true);
    expect(player.snapshot().playing).toBe(false);
    expect(player.snapshot().error).toBe("");
    expect(pause).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);
    expect(requests).toEqual([
      `GET /api/v1/recordings/${first.recording_id}/playback-sources`,
      ...Array<string>(257).fill("POST /api/v1/sync"),
    ]);
    await act(async () => player.setPlaying(true));
    expect(player.snapshot().playing).toBe(true);
    expect(media.current()).toBe(audio);
    expect(player.snapshot().current?.id).toBe(first.recording_id);
    expect(submitted).toHaveLength(257);
  } finally {
    view.unmount();
    await act(async () => {
      releaseFirst?.();
      await Promise.allSettled(pending);
    });
    if (savedClient === null) localStorage.removeItem("hito-device");
    else localStorage.setItem("hito-device", savedClient);
    expect(document.querySelectorAll("audio")).toHaveLength(0);
    expect(media.created.every((audio) => audio.paused)).toBe(true);
  }
});
