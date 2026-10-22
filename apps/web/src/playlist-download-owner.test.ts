import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import PlaylistPage from "./pages/PlaylistPage";
import { NoticeProvider } from "./components/Notices";
import { PlaybackProvider, usePlayer } from "./player/PlaybackProvider";
import type { Player } from "./player/model";
import type { PlaylistDownloadStatus, PlaylistPageProps } from "./types";
import { copiedPlaylistPage, recordingB } from "./fixtures/playlistPages";
import { heldResponse } from "./test/async";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

test("a held removal cannot cross accounts and an unmounted status read cannot restart playlist download work", async () => {
  vi.useFakeTimers();
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const warnings = vi.spyOn(console, "warn").mockImplementation(() => {});
  const first = copiedPlaylistPage;
  const current: PlaylistPageProps = {
    ...copiedPlaylistPage,
    user: { id: "bob", username: "Bob", admin: false },
    detail: {
      ...copiedPlaylistPage.detail,
      playlist: {
        ...copiedPlaylistPage.detail.playlist,
        id: "23a6f43c-044d-45ca-b264-edf0f66097aa",
        name: "Current account playlist",
      },
    },
  };
  const firstRoot = "ac5eb569-027b-40fa-8729-99b37596fb54";
  const currentRoot = "923d7f13-f6bf-4496-8456-7b51beeecc06";
  const firstPath = `/api/v1/playlists/${first.detail.playlist.id}/download`;
  const currentPath = `/api/v1/playlists/${current.detail.playlist.id}/download`;
  const removalPath = `/api/v1/downloads/${firstRoot}`;
  function status(
    page: PlaylistPageProps,
    retention: string,
  ): PlaylistDownloadStatus {
    return {
      retention_id: retention,
      playlist_id: page.detail.playlist.id,
      host_device_id: "owned-download-host",
      desired_revision: 1,
      reconciled_revision: 1,
      entries: {
        items: page.detail.entries.map((entry) => {
          const ready =
            entry.recording_id !== null &&
            (retention === firstRoot || entry.recording_id === recordingB);
          return {
            entry_id: entry.entry_id,
            recording_id: entry.recording_id,
            status:
              entry.recording_id === null
                ? "unavailable"
                : ready
                  ? "ready"
                  : "queued",
            source_id: ready
              ? entry.recording_id === recordingB
                ? "818e755f-3b13-4a1a-aa64-8216d09494f9"
                : "c9e493ef-4d4d-47f9-bc3a-30aec947bef7"
              : null,
            error: entry.recording_id === null ? "unavailable_item" : null,
          };
        }),
        next_cursor: null,
      },
    };
  }
  const oldRemoval = heldResponse();
  const focusedRead = heldResponse();
  let currentReads = 0;
  const fetch = vi.fn((url: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    const account = new Headers(options?.headers).get("x-hito-account");
    if (url === firstPath && method === "GET" && account === first.user.id)
      return Promise.resolve(Response.json(status(first, firstRoot)));
    if (url === removalPath && method === "DELETE" && account === first.user.id)
      return oldRemoval.response;
    if (
      url === currentPath &&
      method === "GET" &&
      account === current.user.id
    ) {
      currentReads++;
      return currentReads === 1
        ? Promise.resolve(Response.json(status(current, currentRoot)))
        : focusedRead.response;
    }
    throw new Error("Unexpected request outside playlist download ownership");
  });
  vi.stubGlobal("fetch", fetch);
  const players = new Map<string, Player>();
  function Page(props: PlaylistPageProps) {
    players.set(props.user.id, usePlayer());
    return createElement(PlaylistPage, props);
  }
  function page(props: PlaylistPageProps) {
    return createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account: props.user.id,
        children: createElement(Page, props),
      }),
    );
  }
  const view = render(page(first));
  await act(async () => {});
  fireEvent.click(screen.getByRole("button", { name: "Remove download" }));
  const removal = fetch.mock.calls.find(
    ([, options]) => options?.method === "DELETE",
  )!;
  expect(removal[0]).toBe(removalPath);
  expect(removal[1]?.signal?.aborted).toBe(false);
  expect(fetch).toHaveBeenCalledTimes(2);

  view.rerender(page(current));
  expect(removal[1]?.signal?.aborted).toBe(true);
  await act(async () => {});
  expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
    current.detail.playlist.name,
  );
  const action = screen.getByRole("button", { name: "Remove download" });
  const progress = screen.getByRole("progressbar", {
    name: "Playlist download progress",
  });
  const table = screen.getByRole("table", { name: "Playlist entries" });
  const rows = () =>
    within(table)
      .getAllByRole("row")
      .slice(1)
      .map((row) => row.textContent);
  const contents = rows();
  const player = players.get(current.user.id)!;
  const playback = player.snapshot();
  expect(progress.getAttribute("value")).toBe("1");
  expect(progress.getAttribute("max")).toBe("4");
  expect(screen.getByText(/1 of 4 entries saved/)).toBeTruthy();
  expect(
    fetch.mock.calls.map(([url, options]) => [url, options?.method]),
  ).toEqual([
    [firstPath, "GET"],
    [removalPath, "DELETE"],
    [currentPath, "GET"],
  ]);
  expect(fetch.mock.calls[2][1]).toMatchObject({
    credentials: "same-origin",
    headers: { "x-hito-account": current.user.id, "x-hito-request": "1" },
    signal: expect.any(AbortSignal),
  });
  const beforeLate = fetch.mock.calls.length;
  await act(async () =>
    oldRemoval.resolve(new Response(null, { status: 204 })),
  );
  expect(fetch).toHaveBeenCalledTimes(beforeLate);
  expect(screen.getByRole("button", { name: "Remove download" })).toBe(action);
  expect((action as HTMLButtonElement).disabled).toBe(false);
  expect(screen.queryByRole("button", { name: "Download" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Retry removal" })).toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();
  expect(
    screen.getByRole("progressbar", { name: "Playlist download progress" }),
  ).toBe(progress);
  expect(progress.getAttribute("value")).toBe("1");
  expect(rows()).toEqual(contents);
  expect(player.snapshot()).toEqual(playback);
  expect(player.queueSnapshot().total).toBe(0);

  await act(async () => {
    window.dispatchEvent(new Event("focus"));
  });
  expect(currentReads).toBe(2);
  const focused = fetch.mock.calls[fetch.mock.calls.length - 1];
  expect(focused[0]).toBe(currentPath);
  expect(focused[1]).toMatchObject({
    headers: { "x-hito-account": current.user.id },
    signal: expect.any(AbortSignal),
  });
  expect(focused[1]?.signal?.aborted).toBe(false);
  const beforeUnmount = fetch.mock.calls.length;
  view.unmount();
  expect(focused[1]?.signal?.aborted).toBe(true);
  expect(vi.getTimerCount()).toBe(0);
  expect(document.querySelectorAll("audio")).toHaveLength(0);
  const privateFailure = "private late playlist download status failure";
  await act(async () => {
    focusedRead.resolve(
      Response.json({ error: privateFailure }, { status: 500 }),
    );
    await vi.advanceTimersByTimeAsync(60_000);
    window.dispatchEvent(new Event("focus"));
  });
  expect(fetch).toHaveBeenCalledTimes(beforeUnmount);
  expect(vi.getTimerCount()).toBe(0);
  expect(screen.queryByRole("heading", { level: 1 })).toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();
  expect(document.body.textContent).not.toContain(privateFailure);
  expect(JSON.stringify(warnings.mock.calls)).not.toContain(privateFailure);
  expect(player.queueSnapshot().total).toBe(0);
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelectorAll("audio")).toHaveLength(0);
});
