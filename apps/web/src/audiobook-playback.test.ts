// @vitest-environment jsdom
import { createElement, type ReactNode } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import AudiobookPage from "./pages/AudiobookPage";
import { audiobookPages } from "./fixtures/audiobooks";
import { audiobookPlayer } from "./fixtures/audiobookPlayback";
import type { Player } from "./player/model";

const boundaries = vi.hoisted(() => ({ player: null as Player | null }));
vi.mock("./player/PlaybackProvider", () => ({
  usePlayer: () => boundaries.player,
}));
vi.mock("@inertiajs/react", () => ({
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

const request = vi.fn<typeof globalThis.fetch>();
beforeEach(() => {
  vi.clearAllMocks();
  vi.spyOn(console, "info").mockImplementation(() => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.spyOn(console, "debug").mockImplementation(() => {});
  vi.stubGlobal("fetch", request);
});
afterEach(() => {
  cleanup();
  boundaries.player?.clear();
  boundaries.player = null;
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("explicit chapter playback checks the owned book and selects the exact repeated occurrence", async () => {
  const { detail: page } = audiobookPages();
  const { player, media, resolve, progress } = audiobookPlayer();
  boundaries.player = player;
  const [first, second, repeated, retained] = page.detail.chapters;
  const checked = {
    ...page.detail,
    chapters: [retained, first, repeated, { ...second, available: false }],
  };
  request.mockImplementation(async (path, options) => {
    expect(path).toBe(`/api/v1/audiobooks/${page.detail.playlist.id}`);
    expect(options?.method).toBe("GET");
    expect(options?.credentials).toBe("same-origin");
    expect(new Headers(options?.headers).get("x-hito-account")).toBe(
      page.user.id,
    );
    expect(options?.signal).toBeInstanceOf(AbortSignal);
    return Response.json(checked);
  });
  render(createElement(AudiobookPage, page));
  expect(request).not.toHaveBeenCalled();
  expect(resolve).not.toHaveBeenCalled();
  expect(progress).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", { name: "Play chapter 3 from beginning" }),
  );
  await waitFor(() => expect(player.snapshot().playing).toBe(true));
  const queue = await player.queuePage(0, 10);
  expect(queue.entries.map((entry) => entry.recording.id)).toEqual([
    first.recording_id,
    repeated.recording_id,
    second.recording_id,
  ]);
  expect(queue.entries[0].id).not.toBe(queue.entries[1].id);
  expect(queue.entries[2].recording.available).toBe(false);
  expect(player.queueSnapshot().currentIndex).toBe(1);
  expect(player.queueSnapshot().currentEntry).toBe(queue.entries[1].id);
  media.dispatchEvent(new Event("loadedmetadata"));
  expect(media.currentTime).toBe(0);
  expect(progress).not.toHaveBeenCalled();
  expect(request).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("button", { name: /Continue/ })).toBeNull();
});

test("explicit bookmark playback checks the exact marker and starts its repeated chapter at the saved milliseconds", async () => {
  const { detail: page } = audiobookPages();
  const { player, media, progress } = audiobookPlayer();
  boundaries.player = player;
  page.detail.bookmarks[0].position_ms = 35_125;
  request.mockResolvedValue(Response.json(page.detail));
  render(createElement(AudiobookPage, page));
  expect(request).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Play bookmark" }));
  await waitFor(() => expect(player.snapshot().playing).toBe(true));
  expect(player.queueSnapshot().currentIndex).toBe(2);
  media.dispatchEvent(new Event("loadedmetadata"));
  expect(media.currentTime).toBe(35.125);
  expect(progress).not.toHaveBeenCalled();
  expect(request).toHaveBeenCalledTimes(1);
  expect(screen.queryByText(/Playback started|Bookmark playing/)).toBeNull();
});

test.each(["removed", "recording", "null", "unavailable"] as const)(
  "a checked %s chapter cannot retarget another occurrence of its recording",
  async (change) => {
    const { detail: page } = audiobookPages();
    const { player, resolve, progress } = audiobookPlayer();
    boundaries.player = player;
    const target = page.detail.chapters[2];
    const checked = {
      ...page.detail,
      chapters: page.detail.chapters.flatMap((chapter) => {
        if (chapter.entry_id !== target.entry_id) return [chapter];
        if (change === "removed") return [];
        return [
          {
            ...chapter,
            recording_id:
              change === "null"
                ? null
                : change === "recording"
                  ? page.detail.chapters[1].recording_id
                  : chapter.recording_id,
            available: change !== "unavailable",
          },
        ];
      }),
    };
    request.mockResolvedValue(Response.json(checked));
    render(createElement(AudiobookPage, page));
    fireEvent.click(
      screen.getByRole("button", { name: "Play chapter 3 from beginning" }),
    );
    expect((await screen.findByRole("alert")).textContent).toContain(
      "no longer available",
    );
    expect(resolve).not.toHaveBeenCalled();
    expect(progress).not.toHaveBeenCalled();
    expect(player.queueSnapshot().total).toBe(0);
    expect(request).toHaveBeenCalledTimes(1);
  },
);

test.each([
  "removed",
  "identity",
  "book",
  "entry",
  "recording",
  "position",
] as const)(
  "a checked bookmark with changed %s cannot replace its explicit anchor",
  async (change) => {
    const { detail: page } = audiobookPages();
    const { player, resolve } = audiobookPlayer();
    boundaries.player = player;
    const bookmark = page.detail.bookmarks[0];
    const checked = {
      ...page.detail,
      bookmarks:
        change === "removed"
          ? []
          : [
              {
                ...bookmark,
                id: change === "identity" ? "another-marker" : bookmark.id,
                audiobook_id:
                  change === "book" ? "another-book" : bookmark.audiobook_id,
                entry_id:
                  change === "entry"
                    ? page.detail.chapters[0].entry_id
                    : bookmark.entry_id,
                recording_id:
                  change === "recording"
                    ? page.detail.chapters[1].recording_id!
                    : bookmark.recording_id,
                position_ms:
                  change === "position"
                    ? bookmark.position_ms + 125
                    : bookmark.position_ms,
              },
            ],
    };
    request.mockResolvedValue(Response.json(checked));
    render(createElement(AudiobookPage, page));
    fireEvent.click(screen.getByRole("button", { name: "Play bookmark" }));
    expect((await screen.findByRole("alert")).textContent).toContain(
      "no longer available",
    );
    expect(resolve).not.toHaveBeenCalled();
    expect(player.queueSnapshot().total).toBe(0);
    expect(request).toHaveBeenCalledTimes(1);
  },
);

test("unavailable, null and mismatched rendered anchors offer no playback action or request", () => {
  const { detail: page, bookmark, removedChapterBookmark } = audiobookPages();
  const { player, resolve, progress } = audiobookPlayer();
  boundaries.player = player;
  page.detail.chapters[2].available = false;
  page.detail.bookmarks = [
    bookmark,
    removedChapterBookmark,
    {
      ...bookmark,
      id: "mismatched-marker",
      entry_id: page.detail.chapters[1].entry_id,
    },
  ];
  render(createElement(AudiobookPage, page));
  const rows = within(
    screen.getByRole("table", { name: "Chapters" }),
  ).getAllByRole("row");
  expect(within(rows[3]).queryByRole("button", { name: /Play/ })).toBeNull();
  expect(within(rows[4]).queryByRole("button", { name: /Play/ })).toBeNull();
  for (const item of within(
    screen.getByRole("region", { name: "Bookmarks" }),
  ).getAllByRole("listitem"))
    expect(
      within(item).queryByRole("button", { name: "Play bookmark" }),
    ).toBeNull();
  expect(request).not.toHaveBeenCalled();
  expect(resolve).not.toHaveBeenCalled();
  expect(progress).not.toHaveBeenCalled();
});

test("navigation after handoff keeps audio and progress with the existing recording playback owner", async () => {
  const { detail: page } = audiobookPages();
  const { player, media, progress } = audiobookPlayer();
  boundaries.player = player;
  request.mockResolvedValue(Response.json(page.detail));
  const view = render(createElement(AudiobookPage, page));
  fireEvent.click(screen.getByRole("button", { name: "Play bookmark" }));
  await waitFor(() => expect(player.snapshot().playing).toBe(true));
  media.dispatchEvent(new Event("loadedmetadata"));
  const queue = player.queueSnapshot();
  expect(progress).not.toHaveBeenCalled();
  view.unmount();
  expect(media.paused).toBe(false);
  expect(player.queueSnapshot()).toBe(queue);
  media.currentTime = 50;
  media.dispatchEvent(new Event("timeupdate"));
  expect(progress).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({ id: page.detail.chapters[2].recording_id }),
    50,
    1,
  );
  expect(request).toHaveBeenCalledTimes(1);
});
