import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import AudiobookPage from "./pages/AudiobookPage";
import { audiobookPages, heldAudiobookBody } from "./fixtures/audiobooks";
import { bookmarkResult, enterBookmark } from "./fixtures/audiobookBookmarks";
import { audiobookPlayer } from "./fixtures/audiobookPlayback";
import type { Player } from "./player/model";
import type { AudiobookDetail, SyncBatch } from "./shared/types";

const boundaries = vi.hoisted(() => ({ player: null as Player | null }));
vi.mock("./player/PlaybackProvider", () => ({
  usePlayer: () => boundaries.player,
}));
vi.mock("@inertiajs/react", () => ({
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

beforeEach(() => {
  vi.spyOn(console, "info").mockImplementation(() => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.spyOn(console, "debug").mockImplementation(() => {});
});
afterEach(() => {
  cleanup();
  boundaries.player?.clear();
  boundaries.player = null;
});

test("same-book raw props retire a held playback check while the verified bookmark editor retains its old detail", async () => {
  const { detail: page } = audiobookPages();
  const target = page.detail.chapters[2];
  const replacement = {
    ...page,
    detail: {
      ...page.detail,
      chapters: page.detail.chapters.filter(
        (chapter) => chapter.entry_id !== target.entry_id,
      ),
    },
  };
  const { player, resolve, progress } = audiobookPlayer();
  boundaries.player = player;
  await player.playQueue([
    {
      id: "existing-recording",
      title: "Existing selection",
      artist: "Fixture",
      duration_ms: 120_000,
      available: true,
    },
  ]);
  const queue = await player.queuePage(0, 10);
  const selected = player.queueSnapshot();
  resolve.mockClear();
  progress.mockClear();
  let batch: SyncBatch | undefined;
  let verified: AudiobookDetail | undefined;
  let playback: ReturnType<typeof heldAudiobookBody> | undefined;
  let validation: ReturnType<typeof heldAudiobookBody> | undefined;
  let playbackSignal: AbortSignal | undefined;
  const request = vi.fn<typeof globalThis.fetch>(async (path, options) => {
    expect(new Headers(options?.headers).get("x-hito-account")).toBe(
      page.user.id,
    );
    if (path === "/api/v1/sync" && options?.method === "POST" && !batch) {
      batch = JSON.parse(String(options.body)) as SyncBatch;
      return Response.json({
        acknowledged: batch.operations.map((operation) => operation.id),
      });
    }
    if (
      path === `/api/v1/audiobooks/${page.detail.playlist.id}` &&
      options?.method === "GET" &&
      batch
    ) {
      if (!verified) {
        verified = bookmarkResult(page.detail, batch);
        return Response.json(verified);
      }
      if (!playback) {
        playbackSignal = options.signal as AbortSignal;
        playback = heldAudiobookBody(verified);
        return playback.response;
      }
      if (!validation) {
        validation = heldAudiobookBody({
          ...verified,
          chapters: replacement.detail.chapters,
        });
        return validation.response;
      }
    }
    throw new Error("Unexpected verified-editor playback request");
  });
  vi.stubGlobal("fetch", request);
  const view = render(createElement(AudiobookPage, page));
  try {
    enterBookmark(page, "Verified editor marker");
    fireEvent.click(screen.getByRole("button", { name: "Save bookmark" }));
    await screen.findByText("Bookmark saved.", { exact: true });
    expect(request).toHaveBeenCalledTimes(2);
    fireEvent.click(
      screen.getByRole("button", {
        name: "Play chapter 3 from beginning",
      }),
    );
    await waitFor(() => expect(playback?.response.bodyUsed).toBe(true));
    expect(playbackSignal?.aborted).toBe(false);
    view.rerender(createElement(AudiobookPage, replacement));
    expect(playbackSignal?.aborted).toBe(true);
    await waitFor(() => expect(validation?.response.bodyUsed).toBe(true));
    expect(
      document.getElementById(`chapter-${target.entry_id}`),
    ).not.toBeNull();
    await act(async () => playback?.release());
    expect(resolve).not.toHaveBeenCalled();
    expect(progress).not.toHaveBeenCalled();
    expect(await player.queuePage(0, 10)).toEqual(queue);
    expect(player.queueSnapshot()).toBe(selected);
    expect(screen.queryByRole("alert")).toBeNull();
    await act(async () => validation?.release());
    expect(document.getElementById(`chapter-${target.entry_id}`)).toBeNull();
    expect(resolve).not.toHaveBeenCalled();
    expect(player.queueSnapshot()).toBe(selected);
    expect(request).toHaveBeenCalledTimes(4);
  } finally {
    view.unmount();
    await act(async () => {
      await playback?.dispose();
      await validation?.dispose();
    });
  }
});
