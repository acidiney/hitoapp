import type { Schema } from "../../../shared/schema";
import { createElement } from "react";
import { act, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { NoticeBanner, NoticeProvider } from "../../../shared/ui/Notices";
import { audiobookPages } from "../fixtures/audiobooks";
import { audiobookContinuationMedia } from "../fixtures/audiobookContinuationMedia";
import {
  PlaybackProvider,
  usePlayer,
} from "../../playback/components/PlaybackProvider";
import type { Player } from "../../playback/model";

test("a rejected recording checkpoint does not starve the next chapter or retry a definitive denial", async () => {
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
  const sources: Schema<"Source">[] = chapters.map((chapter, index) => ({
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
  const privateError =
    "Private removed title at https://private.invalid/audio?token=secret";
  const submitted: { raw: string; batch: Schema<"SyncBatch"> }[] = [];
  const unexpected: string[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, options?: RequestInit) => {
      expect(options?.credentials).toBe("same-origin");
      expect(new Headers(options?.headers).get("x-hito-account")).toBe(
        page.user.id,
      );
      expect(new Headers(options?.headers).get("x-hito-request")).toBe("1");
      expect(options?.signal).toBeInstanceOf(AbortSignal);
      for (const source of sources)
        if (
          input ===
            `/api/v1/recordings/${source.recording_id}/playback-sources` &&
          options?.method === "GET"
        )
          return Response.json([source]);
      if (input === "/api/v1/sync" && options?.method === "POST") {
        const raw = String(options.body);
        const batch = JSON.parse(raw) as Schema<"SyncBatch">;
        submitted.push({ raw, batch });
        const action = batch.operations[0].action;
        expect(action.kind).toBe("progress_set");
        if (
          action.kind === "progress_set" &&
          action.recording_id === chapters[0].recording_id
        )
          return Response.json({ error: privateError }, { status: 404 });
        return Response.json({
          acknowledged: batch.operations.map(({ id }) => id),
        });
      }
      unexpected.push(String(input));
      throw new Error("Progress denial requested an unowned operation");
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
  const checkpoint = (index: number): Schema<"Action">[] => [
    {
      kind: "progress_set",
      recording_id: chapters[index].recording_id!,
      position_ms: 0,
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
    const denial = screen.getByRole("status").textContent;
    const queue = await player.queuePage(0, 10);
    const prepared = media.prepared(sources[1].id)!;
    expect(prepared).toBeDefined();
    await act(async () => media.ready(prepared));
    await act(async () => player.selectEntry(queue.entries[1].id));
    const second = media.current();
    expect(second).toBe(prepared);
    await act(async () => media.observePlaying(second));
    expect(
      submitted.some(({ batch }) => {
        const action = batch.operations[0].action;
        return (
          action.kind === "progress_set" &&
          action.recording_id === chapters[1].recording_id
        );
      }),
    ).toBe(true);
    expect(
      submitted.map(({ batch }) =>
        batch.operations.map(({ action }) => action),
      ),
    ).toEqual([checkpoint(0), checkpoint(1)]);
    const message = "Listening progress could not be saved for this item.";
    expect(denial).toBe(message);
    expect(screen.getByRole("status").textContent).toBe(message);
    expect(player.snapshot().playing).toBe(true);
    expect(player.snapshot().current?.id).toBe(chapters[1].recording_id);
    expect(player.snapshot().error).toBe("");
    expect(submitted.every(({ batch }) => batch.device_id === client)).toBe(
      true,
    );
    const ids = submitted.flatMap(({ batch }) =>
      batch.operations.map(({ id }) => id),
    );
    expect(new Set(ids).size).toBe(4);
    expect(
      submitted.every(({ raw, batch }) => raw === JSON.stringify(batch)),
    ).toBe(true);
    for (const { raw } of submitted) {
      expect(raw).not.toContain(privateError);
      for (const chapter of chapters) {
        expect(raw).not.toContain(chapter.title);
        expect(raw).not.toContain(chapter.artist);
      }
    }
    const logs = JSON.stringify(
      [console.info, console.warn, console.debug].flatMap(
        (method) => vi.mocked(method).mock.calls,
      ),
    );
    expect(logs).not.toContain(privateError);
    expect(logs).not.toContain("token=secret");
    expect(vi.getTimerCount()).toBe(0);
    await act(async () => vi.advanceTimersByTimeAsync(30_000));
    expect(submitted).toHaveLength(2);
    expect(unexpected).toEqual([]);
  } finally {
    view.unmount();
    await act(async () => {});
    if (savedClient === null) localStorage.removeItem("hito-device");
    else localStorage.setItem("hito-device", savedClient);
    expect(document.querySelectorAll("audio")).toHaveLength(0);
    expect(media.created.every((audio) => audio.paused)).toBe(true);
  }
});
