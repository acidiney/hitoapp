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
import { afterEach, expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import PlaylistPage from "./pages/PlaylistPage";
import { NoticeProvider } from "./components/Notices";
import { PlaybackProvider, usePlayer } from "./player/PlaybackProvider";
import type { Player } from "./player/model";
import type { SyncBatch } from "./types";
import { unavailablePlaylistPages } from "./fixtures/playlistPages";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({
    children,
    href,
    ...attributes
  }: {
    children: ReactNode;
    href: string;
  }) => createElement("a", { href, ...attributes }, children),
}));

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
  localStorage.clear();
});

test("wholly retained and unavailable playlists remain visible and editable without starting playback", async () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  for (const props of unavailablePlaylistPages) {
    vi.mocked(router.reload).mockClear();
    const downloadPath = `/api/v1/playlists/${props.detail.playlist.id}/download`;
    const fetch = vi
      .fn()
      .mockImplementation(async (path: string, options: RequestInit) => {
        if (
          path === downloadPath &&
          options.method === "GET" &&
          new Headers(options.headers).get("x-hito-account") === props.user.id
        )
          return Response.json({ error: "Not found" }, { status: 404 });
        if (path !== "/api/v1/sync")
          throw new Error("Unavailable rows must not request sources");
        const batch: SyncBatch = JSON.parse(String(options.body));
        return Response.json({
          acknowledged: batch.operations.map((operation) => operation.id),
        });
      });
    vi.stubGlobal("fetch", fetch);
    let player!: Player;
    function Page() {
      player = usePlayer();
      return createElement(PlaylistPage, props);
    }
    const view = render(
      createElement(
        NoticeProvider,
        null,
        createElement(PlaybackProvider, {
          account: props.user.id,
          children: createElement(Page),
        }),
      ),
    );
    const table = screen.getByRole("table", { name: "Playlist entries" });
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows).toHaveLength(2);
    expect(screen.getByText("2 entries", { exact: true })).toBeTruthy();
    expect(screen.queryByText("This playlist is ready for music")).toBeNull();
    expect(screen.getByText(/Curated by the fixture owner/)).toBeTruthy();
    const toolbar = screen.getByRole("button", { name: "Play playlist" });
    expect(toolbar.hasAttribute("disabled")).toBe(true);
    fireEvent.click(toolbar);
    for (const [index, entry] of props.detail.entries.entries()) {
      const row = within(rows[index]);
      expect(row.getByText(entry.title)).toBeTruthy();
      if (entry.recording_id == null) {
        expect(row.queryAllByRole("link")).toHaveLength(0);
        expect(row.getAllByRole("button")).toHaveLength(1);
        expect(
          row.queryByRole("button", { name: /Play|Like|Unlike|Details/ }),
        ).toBeNull();
        expect(rows[index].querySelector("img")).toBeNull();
        expect(row.getAllByRole("cell")[4].textContent?.trim()).toBe("—");
        expect(
          row.getByText(
            entry.origin?.kind === "unknown"
              ? "Unknown item from its provider"
              : "Unavailable from its provider",
          ),
        ).toBeTruthy();
      } else {
        const rowPlay = row.getByRole("button", {
          name: `Play ${entry.title}`,
        });
        const titlePlay = row.getByRole("button", { name: entry.title });
        expect(rowPlay.hasAttribute("disabled")).toBe(true);
        expect(titlePlay.hasAttribute("disabled")).toBe(true);
        fireEvent.click(rowPlay);
        fireEvent.click(titlePlay);
        expect(row.getByText("Unavailable on this device")).toBeTruthy();
        expect(
          row
            .getByRole("link", { name: `Details for ${entry.title}` })
            .getAttribute("href"),
        ).toBe(`/recordings/${entry.recording_id}`);
        expect(
          row
            .getByRole("button", { name: `Like ${entry.title}` })
            .hasAttribute("disabled"),
        ).toBe(false);
      }
    }
    expect(player.queueSnapshot().total).toBe(0);
    expect((await player.queuePage(0, 10)).entries).toEqual([]);
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(fetch.mock.calls[0]).toEqual([
      downloadPath,
      expect.objectContaining({
        method: "GET",
        credentials: "same-origin",
        headers: expect.objectContaining({ "x-hito-account": props.user.id }),
        signal: expect.any(AbortSignal),
      }),
    ]);
    expect(
      fetch.mock.calls.filter(([, options]) => options?.method !== "GET"),
    ).toHaveLength(0);
    expect(play).not.toHaveBeenCalled();
    const removed = props.detail.entries[1];
    fireEvent.click(
      within(rows[1]).getByRole("button", {
        name: `Remove ${removed.title} from playlist`,
      }),
    );
    await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(1));
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(fetch.mock.calls[1][0]).toBe("/api/v1/sync");
    expect(JSON.parse(fetch.mock.calls[1][1].body).operations).toEqual([
      {
        id: expect.any(String),
        action: {
          kind: "entry_remove",
          playlist_id: props.detail.playlist.id,
          entry_id: removed.entry_id,
        },
      },
    ]);
    expect(player.queueSnapshot().total).toBe(0);
    view.unmount();
  }
});
