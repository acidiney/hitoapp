// @vitest-environment jsdom
import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import PlaylistPage from "./pages/PlaylistPage";
import { NoticeProvider } from "./components/Notices";
import { PlaybackProvider, usePlayer } from "./player/PlaybackProvider";
import type { Player } from "./player/model";
import type { PlaylistDownloadStatus } from "./types";
import {
  copiedPlaylistPage as props,
  recordingA,
  recordingB,
} from "./fixtures/playlistPages";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
  localStorage.clear();
});

test("removing a playlist download releases only its displayed retention and preserves entries and playback", async () => {
  vi.useFakeTimers();
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const path = `/api/v1/playlists/${props.detail.playlist.id}/download`;
  const retention = "d813f9bf-c566-449c-9e8b-45a440225617";
  const removePath = `/api/v1/downloads/${retention}`;
  const active: PlaylistDownloadStatus = {
    retention_id: retention,
    playlist_id: props.detail.playlist.id,
    host_device_id: "owned-download-host",
    desired_revision: 1,
    reconciled_revision: 1,
    entries: {
      items: props.detail.entries.map((entry) => ({
        entry_id: entry.entry_id,
        recording_id: entry.recording_id,
        status: entry.recording_id === null ? "unavailable" : "ready",
        source_id:
          entry.recording_id === null
            ? null
            : entry.recording_id === recordingB
              ? "f459e89b-aeb9-4375-80a8-9240741824bc"
              : "a44b2090-a6a0-48a3-bcba-380069d086e4",
        error: entry.recording_id === null ? "unavailable_item" : null,
      })),
      next_cursor: null,
    },
  };
  let retained = true;
  let finishRemove!: (response: Response) => void;
  const fetch = vi.fn(async (url: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (url === path && method === "GET")
      return retained
        ? Response.json(active)
        : Response.json({ error: "Not found" }, { status: 404 });
    if (url === removePath && method === "DELETE")
      return new Promise<Response>((resolve) => {
        finishRemove = (response) => {
          retained = false;
          resolve(response);
        };
      });
    throw new Error("Unexpected request outside playlist download removal");
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
  await act(async () => {});
  const remove = screen.getByRole("button", { name: "Remove download" });
  const table = screen.getByRole("table", { name: "Playlist entries" });
  const rows = () => within(table).getAllByRole("row").slice(1);
  const contents = rows().map((row) => row.textContent);
  const recordingLinks = () =>
    within(table)
      .getAllByRole("link")
      .map((link) => link.getAttribute("href"));
  const originalLinks = [recordingA, recordingB, recordingA].map(
    (recording) => `/recordings/${recording}`,
  );
  const playback = player.snapshot();
  expect(recordingLinks()).toEqual(originalLinks);
  expect(contents).toHaveLength(4);
  expect(screen.getByText(/copies are saved on this server/i)).toBeTruthy();
  expect(screen.getByText(/browser.*needs.*connection.*server/i)).toBeTruthy();
  expect(
    fetch.mock.calls.map(([url, options]) => [url, options?.method]),
  ).toEqual([[path, "GET"]]);

  act(() => {
    fireEvent.click(remove);
    fireEvent.click(remove);
  });
  const writes = fetch.mock.calls.filter(
    ([, options]) => options?.method !== "GET",
  );
  expect(writes).toHaveLength(1);
  expect(writes[0][0]).toBe(removePath);
  expect(writes[0][1]).toMatchObject({
    method: "DELETE",
    credentials: "same-origin",
    headers: { "x-hito-account": props.user.id, "x-hito-request": "1" },
    signal: expect.any(AbortSignal),
  });
  expect(writes[0][1]?.body).toBeUndefined();
  expect(writes[0][1]?.signal?.aborted).toBe(false);
  expect((remove as HTMLButtonElement).disabled).toBe(true);
  expect(screen.queryByRole("button", { name: "Download" })).toBeNull();
  expect(rows().map((row) => row.textContent)).toEqual(contents);
  expect(recordingLinks()).toEqual(originalLinks);
  expect(player.snapshot()).toEqual(playback);
  expect(player.queueSnapshot().total).toBe(0);
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();

  await act(async () => finishRemove(new Response(null, { status: 204 })));
  expect(screen.getByRole("button", { name: "Download" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Remove download" })).toBeNull();
  expect(
    screen.queryByRole("progressbar", { name: "Playlist download progress" }),
  ).toBeNull();
  expect(
    fetch.mock.calls.map(([url, options]) => [url, options?.method]),
  ).toEqual([
    [path, "GET"],
    [removePath, "DELETE"],
    [path, "GET"],
  ]);
  expect(screen.getByRole("table", { name: "Playlist entries" })).toBe(table);
  expect(rows().map((row) => row.textContent)).toEqual(contents);
  expect(recordingLinks()).toEqual(originalLinks);
  expect(player.snapshot()).toEqual(playback);
  expect(player.queueSnapshot().total).toBe(0);
  expect(router.reload).not.toHaveBeenCalled();
  expect(router.visit).not.toHaveBeenCalled();
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
  view.unmount();
  expect(vi.getTimerCount()).toBe(0);
  expect(document.querySelectorAll("audio")).toHaveLength(0);
});
