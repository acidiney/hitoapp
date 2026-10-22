import { createElement } from "react";
import { act, render } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { NoticeBanner, NoticeProvider } from "./components/Notices";
import { audiobookPages } from "./fixtures/audiobooks";
import { audiobookContinuationMedia } from "./fixtures/audiobookContinuationMedia";
import { PlaybackProvider, usePlayer } from "./player/PlaybackProvider";
import type { Player } from "./player/model";
import type { Action, Source, SyncBatch } from "./types";

test.each(["request", "acknowledgement body"] as const)(
  "a stalled progress %s expires before the exact checkpoint retries and later chapters follow in order",
  async (stall) => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    vi.spyOn(console, "info").mockImplementation(() => {});
    vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.spyOn(console, "debug").mockImplementation(() => {});
    const savedClient = localStorage.getItem("hito-device");
    const client = "584f9d08-5ce1-417d-b7a6-8c09c3ba7b47";
    localStorage.setItem("hito-device", client);
    const media = audiobookContinuationMedia();
    const { detail: page } = audiobookPages();
    const chapters = page.detail.chapters.slice(0, 2);
    const sources: Source[] = chapters.map((chapter, index) => ({
      id: [
        "76769d98-1b6b-4ec8-8ff7-478c04d7bb23",
        "b6d336ef-fdf8-49eb-b026-e771fae6b8b4",
      ][index],
      recording_id: chapter.recording_id!,
      provider: "plugin",
      device_id: null,
      ready: true,
      label: "Owned audio service",
      can_prepare: false,
    }));
    const submitted: { raw: string; batch: SyncBatch; signal: AbortSignal }[] =
      [];
    const pending = new Set<Promise<Response>>();
    const unexpected: string[] = [];
    let stalledBody: Response | undefined;
    let releaseRetry: (() => void) | undefined;
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
        for (const source of sources)
          if (
            input ===
              `/api/v1/recordings/${source.recording_id}/playback-sources` &&
            options?.method === "GET"
          )
            return Response.json([source]);
        if (input === "/api/v1/sync" && options?.method === "POST") {
          const raw = String(options.body);
          const batch = JSON.parse(raw) as SyncBatch;
          submitted.push({ raw, batch, signal });
          const receipt = () =>
            Response.json({
              acknowledged: batch.operations.map(({ id }) => id),
            });
          if (submitted.length > 2) return receipt();
          if (submitted.length === 1 && stall === "acknowledgement body") {
            let stream!: ReadableStreamDefaultController<Uint8Array>;
            const abort = () => {
              signal.removeEventListener("abort", abort);
              stream.error(
                new DOMException("The body was aborted", "AbortError"),
              );
            };
            stalledBody = new Response(
              new ReadableStream<Uint8Array>({
                start(controller) {
                  stream = controller;
                  stream.enqueue(new TextEncoder().encode("{"));
                  signal.addEventListener("abort", abort, { once: true });
                  if (signal.aborted) abort();
                },
                cancel() {
                  signal.removeEventListener("abort", abort);
                },
              }),
              { headers: { "content-type": "application/json" } },
            );
            return stalledBody;
          }
          let removeAbort!: () => void;
          const response = new Promise<Response>((resolve, reject) => {
            const abort = () =>
              reject(new DOMException("The request was aborted", "AbortError"));
            signal.addEventListener("abort", abort, { once: true });
            removeAbort = () => signal.removeEventListener("abort", abort);
            if (submitted.length === 2) releaseRetry = () => resolve(receipt());
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
        unexpected.push(String(input));
        throw new Error("Progress recovery requested an unowned operation");
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
    const checkpoint = (index: number, position: number): Action[] => [
      {
        kind: "progress_set",
        recording_id: chapters[index].recording_id!,
        position_ms: position,
        speed: 1,
      },
      {
        kind: "audiobook_chapter_set",
        chapter: {
          audiobook_id: page.detail.playlist.id,
          entry_id: chapters[index].entry_id,
          recording_id: chapters[index].recording_id!,
        },
      },
    ];
    try {
      await act(async () =>
        player.playQueue(
          chapters.map((chapter) => ({
            recording: { ...chapter, id: chapter.recording_id! },
            chapter: {
              audiobook_id: page.detail.playlist.id,
              entry_id: chapter.entry_id,
              recording_id: chapter.recording_id!,
            },
          })),
        ),
      );
      const first = media.current();
      await act(async () => {
        media.ready(first);
        media.observePlaying(first);
      });
      expect(submitted).toHaveLength(1);
      if (stall === "acknowledgement body")
        expect(stalledBody?.bodyUsed).toBe(true);
      const next = media.prepared(sources[1].id)!;
      expect(next).toBeDefined();
      await act(async () => {
        media.ready(next);
        media.end(first);
      });
      expect(media.current()).toBe(next);
      await act(async () => media.observePlaying(next));
      expect(player.queueSnapshot().currentIndex).toBe(1);
      expect(player.snapshot().playing).toBe(true);
      expect(submitted).toHaveLength(1);

      await act(async () => vi.advanceTimersByTimeAsync(9_999));
      expect(submitted[0].signal.aborted).toBe(false);
      expect(submitted).toHaveLength(1);
      await act(async () => vi.advanceTimersByTimeAsync(1));
      expect(submitted[0].signal.aborted).toBe(true);
      expect(submitted).toHaveLength(2);
      expect(submitted[1].raw).toBe(submitted[0].raw);
      expect(submitted[1].signal).not.toBe(submitted[0].signal);
      expect(submitted[1].signal.aborted).toBe(false);
      await act(async () => releaseRetry!());
      expect(submitted).toHaveLength(4);
      expect(
        submitted.map(({ batch }) =>
          batch.operations.map(({ action }) => action),
        ),
      ).toEqual([
        checkpoint(0, 0),
        checkpoint(0, 0),
        checkpoint(0, 120_000),
        checkpoint(1, 0),
      ]);
      expect(submitted.every(({ batch }) => batch.device_id === client)).toBe(
        true,
      );
      const ids = submitted
        .filter((_, index) => index !== 1)
        .flatMap(({ batch }) => batch.operations.map(({ id }) => id));
      expect(new Set(ids).size).toBe(6);
      expect(document.querySelector('[role="status"]')).toBeNull();
      expect(player.snapshot().playing).toBe(true);
      expect(player.snapshot().error).toBe("");
      expect(vi.getTimerCount()).toBe(0);
      await act(async () => vi.advanceTimersByTimeAsync(30_000));
      expect(submitted).toHaveLength(4);
      expect(unexpected).toEqual([]);
    } finally {
      view.unmount();
      await act(async () => {
        releaseRetry?.();
        await Promise.allSettled(pending);
      });
      if (savedClient === null) localStorage.removeItem("hito-device");
      else localStorage.setItem("hito-device", savedClient);
      expect(document.querySelectorAll("audio")).toHaveLength(0);
      expect(media.created.every((audio) => audio.paused)).toBe(true);
    }
  },
);
