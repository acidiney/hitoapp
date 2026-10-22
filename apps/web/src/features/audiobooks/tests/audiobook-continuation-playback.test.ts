import { createElement, type ReactNode } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { audiobookPages } from "../fixtures/audiobooks";
import {
  audiobookContinuationMedia,
  chapterAudioPath,
} from "../fixtures/audiobookContinuationMedia";
import AudiobookPage from "../pages/AudiobookPage";
import {
  PlaybackProvider,
  usePlayer,
} from "../../playback/components/PlaybackProvider";
import type { Player } from "../../playback/model";
import type { Action, Source, SyncBatch } from "../../../shared/types";

vi.mock("@inertiajs/react", () => ({
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

const client = "584f9d08-5ce1-417d-b7a6-8c09c3ba7b47";
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

type Request = {
  path: string;
  method: string;
  account: string | null;
  raw: string | undefined;
  signal: AbortSignal;
};
type Submitted = Request & { batch: SyncBatch };

test("observed playback checkpoints the exact A, B, A chapter occurrences in account order", async () => {
  vi.spyOn(console, "info").mockImplementation(() => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.spyOn(console, "debug").mockImplementation(() => {});
  const savedClient = localStorage.getItem("hito-device");
  localStorage.setItem("hito-device", client);
  const media = audiobookContinuationMedia();
  const { detail: page } = audiobookPages();
  page.detail.chapters = page.detail.chapters.slice(0, 3);
  page.detail.bookmarks = [];
  const [first, middle, last] = page.detail.chapters;
  const bookPath = `/api/v1/audiobooks/${page.detail.playlist.id}`;
  const source = (recording: string, id: string): Source => ({
    id,
    recording_id: recording,
    provider: "plugin",
    device_id: null,
    ready: true,
    label: "Owned audio service",
    can_prepare: false,
  });
  const sourceA = source(
    first.recording_id!,
    "76769d98-1b6b-4ec8-8ff7-478c04d7bb23",
  );
  const sourceB = source(
    middle.recording_id!,
    "b6d336ef-fdf8-49eb-b026-e771fae6b8b4",
  );
  const sourcesPath = (recording: string) =>
    `/api/v1/recordings/${recording}/playback-sources`;
  const requests: Request[] = [];
  const unexpected: Request[] = [];
  const submitted: Submitted[] = [];
  const pending = new Set<Promise<Response>>();
  let releaseFirst: (() => void) | undefined;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, options?: RequestInit) => {
      const request: Request = {
        path: String(input),
        method: options?.method ?? "GET",
        account: new Headers(options?.headers).get("x-hito-account"),
        raw: options?.body == null ? undefined : String(options.body),
        signal: options?.signal as AbortSignal,
      };
      requests.push(request);
      expect(options?.credentials).toBe("same-origin");
      expect(request.account).toBe(page.user.id);
      expect(request.signal).toBeInstanceOf(AbortSignal);
      if (request.path === bookPath && request.method === "GET")
        return Response.json(page.detail);
      for (const owned of [sourceA, sourceB])
        if (
          request.path === sourcesPath(owned.recording_id) &&
          request.method === "GET"
        )
          return Response.json([owned]);
      if (request.path === "/api/v1/sync" && request.method === "POST") {
        const batch = JSON.parse(request.raw!) as SyncBatch;
        submitted.push({ ...request, batch });
        const receipt = () =>
          Response.json({
            acknowledged: batch.operations.map((operation) => operation.id),
          });
        if (submitted.length !== 1) return receipt();
        const response = new Promise<Response>((resolve) => {
          releaseFirst = () => resolve(receipt());
        });
        pending.add(response);
        try {
          return await response;
        } finally {
          pending.delete(response);
        }
      }
      unexpected.push(request);
      throw new Error("Chapter playback requested an unowned HTTP operation");
    }),
  );
  let player!: Player;
  function Capture() {
    player = usePlayer();
    return createElement(AudiobookPage, page);
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
  const checkpoint = (chapter: typeof first): Action[] => [
    {
      kind: "progress_set",
      recording_id: chapter.recording_id!,
      position_ms: 0,
      speed: 1,
    },
    {
      kind: "audiobook_chapter_set",
      chapter: {
        audiobook_id: page.detail.playlist.id,
        entry_id: chapter.entry_id,
        recording_id: chapter.recording_id!,
      },
    },
  ];
  const progress = () =>
    submitted.flatMap(({ batch }) =>
      batch.operations.flatMap(({ action }) =>
        action.kind === "progress_set" ? [action] : [],
      ),
    );
  try {
    expect(requests).toEqual([]);
    fireEvent.click(
      screen.getByRole("button", { name: "Play chapter 1 from beginning" }),
    );
    await waitFor(() => expect(player.snapshot().playing).toBe(true));
    expect(requests[0].path).toBe(bookPath);
    const queue = await player.queuePage(0, 10);
    expect(queue.entries.map(({ recording }) => recording.id)).toEqual([
      first.recording_id,
      middle.recording_id,
      last.recording_id,
    ]);
    expect(new Set(queue.entries.map(({ id }) => id)).size).toBe(3);
    for (const entry of queue.entries)
      expect(
        page.detail.chapters.some(({ entry_id }) => entry_id === entry.id),
      ).toBe(false);
    const firstAudio = media.current();
    expect(firstAudio.getAttribute("src")).toBe(chapterAudioPath(sourceA.id));
    expect(firstAudio.readyState).toBe(0);
    expect(firstAudio.currentTime).toBe(0);
    expect(submitted).toEqual([]);

    await act(async () => {
      media.ready(firstAudio);
      media.observePlaying(firstAudio);
    });
    await waitFor(() =>
      expect(
        submitted[0]?.batch.operations.map(({ action }) => action),
      ).toEqual(checkpoint(first)),
    );
    await waitFor(() => expect(media.prepared(sourceB.id)).toBeDefined());
    const middleAudio = media.prepared(sourceB.id)!;
    await act(async () => {
      media.ready(middleAudio);
      media.end(firstAudio);
    });
    await waitFor(() => {
      expect(player.queueSnapshot().currentIndex).toBe(1);
      expect(player.snapshot().playing).toBe(true);
      expect(media.current()).toBe(middleAudio);
    });
    await act(async () => media.observePlaying(middleAudio));
    expect(submitted).toHaveLength(1);

    await act(async () => {
      releaseFirst!();
      await Promise.all(pending);
    });
    await waitFor(() => expect(progress()).toHaveLength(3));
    expect(submitted[2].batch.operations.map(({ action }) => action)).toEqual(
      checkpoint(middle),
    );
    await waitFor(() => expect(media.prepared(sourceA.id)).toBeDefined());
    const lastAudio = media.prepared(sourceA.id)!;
    await act(async () => {
      media.ready(lastAudio);
      media.end(middleAudio);
    });
    await waitFor(() => {
      expect(player.queueSnapshot().currentIndex).toBe(2);
      expect(player.snapshot().playing).toBe(true);
      expect(media.current()).toBe(lastAudio);
    });
    await act(async () => media.observePlaying(lastAudio));
    await waitFor(() => expect(progress()).toHaveLength(5));
    expect(submitted[4].batch.operations.map(({ action }) => action)).toEqual(
      checkpoint(last),
    );
    expect(progress()).toEqual([
      checkpoint(first)[0],
      { ...checkpoint(first)[0], position_ms: 120_000 },
      checkpoint(middle)[0],
      { ...checkpoint(middle)[0], position_ms: 120_000 },
      checkpoint(last)[0],
    ]);
    const expectedChapters = [first, first, middle, middle, last];
    const operationIds: string[] = [];
    submitted.forEach(({ batch, raw }, index) => {
      expect(raw).toBe(JSON.stringify(batch));
      expect(Object.keys(batch).sort()).toEqual(["device_id", "operations"]);
      expect(batch.device_id).toBe(client);
      expect(batch.operations[0].action.kind).toBe("progress_set");
      expect(batch.operations.length).toBeGreaterThanOrEqual(1);
      expect(batch.operations.length).toBeLessThanOrEqual(2);
      for (const operation of batch.operations) {
        expect(Object.keys(operation).sort()).toEqual(["action", "id"]);
        expect(operation.id).toMatch(uuid);
        operationIds.push(operation.id);
        if (operation.action.kind !== "progress_set")
          expect(operation.action).toEqual(
            checkpoint(expectedChapters[index])[1],
          );
      }
    });
    expect(new Set(operationIds).size).toBe(operationIds.length);
    expect((await player.queuePage(0, 10)).entries.map(({ id }) => id)).toEqual(
      queue.entries.map(({ id }) => id),
    );
    expect(player.queueSnapshot().currentEntry).toBe(queue.entries[2].id);
    expect(unexpected).toEqual([]);
    expect(
      requests.filter(({ method }) => method === "GET").map(({ path }) => path),
    ).toEqual([
      bookPath,
      sourcesPath(sourceA.recording_id),
      sourcesPath(sourceB.recording_id),
      sourcesPath(sourceB.recording_id),
      sourcesPath(sourceA.recording_id),
      sourcesPath(sourceA.recording_id),
    ]);
    expect(
      media.loaded.flatMap(({ source }) => (source === null ? [] : [source])),
    ).toEqual([
      chapterAudioPath(sourceA.id),
      chapterAudioPath(sourceB.id),
      chapterAudioPath(sourceA.id),
    ]);
    expect(media.played.map(({ source }) => source)).toEqual([
      chapterAudioPath(sourceA.id),
      chapterAudioPath(sourceB.id),
      chapterAudioPath(sourceA.id),
    ]);
    expect(new Set(media.played.map(({ media: audio }) => audio)).size).toBe(3);
  } finally {
    await act(async () => {
      releaseFirst?.();
      await Promise.all(pending);
    });
    view.unmount();
    await act(async () => {
      await Promise.all(pending);
    });
    if (savedClient === null) localStorage.removeItem("hito-device");
    else localStorage.setItem("hito-device", savedClient);
    expect(document.querySelectorAll("audio")).toHaveLength(0);
    expect(media.created.every((audio) => audio.paused)).toBe(true);
  }
});
