// @vitest-environment jsdom
import { createElement, type ReactNode } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import AudiobookPage from "./pages/AudiobookPage";
import { audiobookPages } from "./fixtures/audiobooks";
import { NativePlayback } from "./player/native/NativePlayback";
import { nativeService } from "./player/native/fixtures/service";
import type { Player } from "./player/model";

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
  boundaries.player = null;
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("native bookmark playback selects the exact duplicate beyond a command chunk before its explicit seek", async () => {
  const { detail: page } = audiobookPages();
  const chapter = page.detail.chapters[0];
  page.detail.chapters = Array.from({ length: 1025 }, (_, index) => ({
    ...chapter,
    entry_id: `chapter-${index}`,
    position: index,
    title: `Private chapter ${index}`,
  }));
  page.detail.bookmarks = [
    {
      ...page.detail.bookmarks[0],
      entry_id: page.detail.chapters[1002].entry_id,
      position_ms: 35_125,
    },
  ];
  const service = nativeService();
  service.state.queue_len = 1025;
  service.queue = async (query) =>
    Response.json({
      revision: query.revision,
      total: 1025,
      offset: query.offset,
      entries: [
        {
          id: `native-entry-${query.offset}`,
          recording: { ...chapter, id: chapter.recording_id },
        },
      ],
    });
  const nativeRequest = vi.mocked(fetch).getMockImplementation()!;
  const reads = vi.fn(async () => Response.json(page.detail));
  vi.stubGlobal(
    "fetch",
    vi.fn<typeof globalThis.fetch>((path, options) => {
      if (path === `/api/v1/audiobooks/${page.detail.playlist.id}`)
        return reads();
      return nativeRequest(path, options);
    }),
  );
  const player = new NativePlayback(page.user.id);
  boundaries.player = player;
  try {
    await player.reconnect();
    render(createElement(AudiobookPage, page));
    expect(reads).not.toHaveBeenCalled();
    expect(service.commands).toHaveLength(0);
    fireEvent.click(screen.getByRole("button", { name: "Play bookmark" }));
    await waitFor(() =>
      expect(service.commands.at(-1)?.action.kind).toBe("resume"),
    );
    expect(service.commands.map((command) => command.action)).toEqual([
      { kind: "pause" },
      {
        kind: "replace",
        recordings: Array(1000).fill(chapter.recording_id),
        start: 0,
      },
      { kind: "append", recordings: Array(25).fill(chapter.recording_id) },
      { kind: "select", entry: "native-entry-1002" },
      { kind: "seek", position_ms: 35_125 },
      { kind: "resume" },
    ]);
    expect(reads).toHaveBeenCalledTimes(1);
    expect(
      service.requests.every((path) => path.startsWith("/api/v1/player")),
    ).toBe(true);
    expect(JSON.stringify(service.commands)).not.toContain("Private chapter");
    expect(screen.queryByText(/Playback started|Bookmark playing/)).toBeNull();
  } finally {
    player.close();
  }
});
