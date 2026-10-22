import { createElement } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  renderHook,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import AudiobookPage from "../pages/AudiobookPage";
import { useAudiobookPlayback } from "../hooks/useAudiobookPlayback";
import { audiobookPages } from "../fixtures/audiobooks";
import { NativePlayback } from "../../playback/native/NativePlayback";
import { nativeService } from "../../playback/fixtures/native/service";
import type { Player } from "../../playback/model";

const boundaries = vi.hoisted(() => ({ player: null as Player | null }));
vi.mock("@inertiajs/react", async () => {
  const { createElement } = await import("react");
  return {
    Link: ({
      children,
      href,
    }: {
      children: import("react").ReactNode;
      href: string;
    }) => createElement("a", { href }, children),
  };
});
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => boundaries.player,
}));

beforeEach(() => {
  vi.spyOn(console, "info").mockImplementation(() => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.spyOn(console, "debug").mockImplementation(() => {});
});
afterEach(() => {
  cleanup();
  boundaries.player = null;
});

function nativeBookmarkScenario(count: number, bookmarked: number) {
  const { detail: page } = audiobookPages();
  const chapter = page.detail.chapters[0];
  page.detail.chapters = Array.from({ length: count }, (_, index) => ({
    ...chapter,
    entry_id: `d19d087c-061b-44fa-966c-${index.toString(16).padStart(12, "0")}`,
    position: index,
    title: `Private chapter ${index}`,
  }));
  page.detail.bookmarks = [
    {
      ...page.detail.bookmarks[0],
      entry_id: page.detail.chapters[bookmarked].entry_id,
      position_ms: 35_125,
    },
  ];
  const service = nativeService();
  const entries = page.detail.chapters.map((entry) => ({
    recording_id: entry.recording_id!,
    chapter: {
      audiobook_id: page.detail.playlist.id,
      entry_id: entry.entry_id,
      recording_id: entry.recording_id!,
    },
  }));
  const nativeEntry = (index: number) =>
    `42230441-aa31-489b-ac8f-${index.toString(16).padStart(12, "0")}`;
  service.state.queue_len = count;
  service.queue = async (query) =>
    Response.json({
      revision: query.revision,
      total: count,
      offset: query.offset,
      entries: [
        {
          id: nativeEntry(query.offset),
          recording: { ...chapter, id: chapter.recording_id },
          chapter: entries[query.offset].chapter,
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
  return { page, service, entries, nativeEntry, reads, player };
}

test("native bookmark playback selects the exact duplicate beyond a command chunk before its explicit seek", async () => {
  const { page, service, entries, nativeEntry, reads, player } =
    nativeBookmarkScenario(1025, 1002);
  try {
    await player.reconnect();
    const playback = renderHook(() =>
      useAudiobookPlayback(page.user.id, page.detail, page.detail),
    );
    expect(reads).not.toHaveBeenCalled();
    expect(service.commands).toHaveLength(0);
    await act(() =>
      playback.result.current.playBookmark(page.detail.bookmarks[0]),
    );
    expect(service.commands.at(-1)?.action.kind).toBe("resume");
    expect(service.commands.map((command) => command.action)).toEqual([
      { kind: "pause" },
      {
        kind: "replace_entries",
        entries: entries.slice(0, 1000),
        start: 0,
      },
      { kind: "append_entries", entries: entries.slice(1000) },
      { kind: "select", entry: nativeEntry(1002) },
      { kind: "seek", position_ms: 35_125 },
      { kind: "resume" },
    ]);
    expect(entries[0].recording_id).toBe(entries[1002].recording_id);
    expect(entries[0].chapter.entry_id).not.toBe(
      entries[1002].chapter.entry_id,
    );
    expect(entries[1002].chapter.entry_id).not.toBe(nativeEntry(1002));
    expect((await player.queuePage(1002, 1)).entries[0]).toMatchObject({
      id: nativeEntry(1002),
      chapter: entries[1002].chapter,
    });
    expect(reads).toHaveBeenCalledTimes(1);
    expect(
      service.requests.every((path) => path.startsWith("/api/v1/player")),
    ).toBe(true);
    expect(JSON.stringify(service.commands)).not.toContain("Private chapter");
    expect(playback.result.current.error).toBe("");
    expect(playback.result.current.pending).toBe(false);
  } finally {
    player.close();
  }
});

test("the audiobook page plays a native bookmark on its exact duplicate chapter without a started message", async () => {
  const { page, service, entries, reads, player } = nativeBookmarkScenario(
    3,
    2,
  );
  try {
    await player.reconnect();
    render(createElement(AudiobookPage, page));
    expect(reads).not.toHaveBeenCalled();
    expect(service.commands).toHaveLength(0);
    fireEvent.click(screen.getByText("Play bookmark"));
    await waitFor(() =>
      expect(service.commands.at(-1)?.action.kind).toBe("resume"),
    );
    expect(service.commands.map((command) => command.action)).toEqual([
      { kind: "pause" },
      { kind: "replace_entries", entries, start: 2 },
      { kind: "seek", position_ms: 35_125 },
      { kind: "resume" },
    ]);
    expect(entries[0].recording_id).toBe(entries[2].recording_id);
    expect(reads).toHaveBeenCalledTimes(1);
    expect(JSON.stringify(service.commands)).not.toContain("Private chapter");
    expect(screen.queryByText(/Playback started|Bookmark playing/)).toBeNull();
  } finally {
    player.close();
  }
});
