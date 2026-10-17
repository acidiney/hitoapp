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
import { PlaybackProvider } from "./player/PlaybackProvider";
import { copiedPlaylistPage } from "./fixtures/playlistPages";
import type { components } from "./generated";
import type { PlaylistPageProps } from "./types";

type Schema = components["schemas"];

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
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

function heldResponse() {
  let resolve!: (response: Response) => void;
  const response = new Promise<Response>((accept) => {
    resolve = accept;
  });
  return { response, resolve };
}

test("a followed playlist retains its entries and reloads once after a refresh with an unchanged token and timestamp", async () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-10-17T03:00:00Z"));
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const props: PlaylistPageProps = {
    ...copiedPlaylistPage,
    detail: {
      ...copiedPlaylistPage.detail,
      playlist: { ...copiedPlaylistPage.detail.playlist, kind: "followed" },
    },
  };
  const ready: Schema["PlaylistFollow"] = {
    playlist_id: props.detail.playlist.id,
    interval: "hourly",
    snapshot: "retained-snapshot",
    last_refreshed_at: Math.floor(Date.now() / 1000),
    next_refresh_at: Math.floor(Date.now() / 1000) + 3600,
    status: "ready",
    error: null,
  };
  const statusPath = `/api/v1/playlists/${ready.playlist_id}/follow`;
  const refreshPath = `${statusPath}/refresh`;
  const accepted = heldResponse();
  const running = heldResponse();
  const completed = heldResponse();
  let statusReads = 0;
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    if (path === statusPath && (options?.method ?? "GET") === "GET") {
      statusReads++;
      if (statusReads === 1) return Promise.resolve(Response.json(ready));
      if (statusReads === 2)
        return Promise.resolve(
          Response.json({
            ...ready,
            status: "queued",
          } satisfies Schema["PlaylistFollow"]),
        );
      if (statusReads === 3) return running.response;
      if (statusReads === 4) return completed.response;
      return Promise.resolve(Response.json(ready));
    }
    if (path === refreshPath && options?.method === "POST")
      return accepted.response;
    throw new Error(
      "Unexpected request outside the followed playlist workflow",
    );
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account: props.user.id,
        children: createElement(PlaylistPage, props),
      }),
    ),
  );
  const refresh = screen.getByRole("button", { name: "Refresh now" });
  await act(async () => {});
  const region = screen.getByRole("region", { name: "Playlist updates" });
  const updates = within(region);
  expect(updates.getByText("Ready", { exact: true })).toBeTruthy();
  expect(updates.getByText("Every hour", { exact: true })).toBeTruthy();
  expect(updates.getByText(/Last checked/)).toBeTruthy();
  expect(updates.getByText(/Next check/)).toBeTruthy();
  for (const timestamp of [ready.last_refreshed_at, ready.next_refresh_at]) {
    const date = new Date(timestamp * 1000).toISOString();
    expect(region.querySelector(`time[datetime="${date}"]`)).not.toBeNull();
  }
  expect(updates.queryByText(ready.snapshot)).toBeNull();
  expect(screen.getByText("Curated by the fixture owner")).toBeTruthy();
  expect(screen.getByText(/Read-only playlist/)).toBeTruthy();
  expect(screen.queryByText(/Editable copy/)).toBeNull();
  expect(
    screen.queryByRole("button", { name: /^Remove .* from playlist$/ }),
  ).toBeNull();
  const table = screen.getByRole("table", { name: "Playlist entries" });
  const rows = () => within(table).getAllByRole("row").slice(1);
  const retainedEntries = rows().map((row) => row.textContent);
  expect(retainedEntries).toHaveLength(props.detail.entries.length);
  expect(statusReads).toBe(1);
  expect(fetch.mock.calls[0]).toEqual([
    statusPath,
    expect.objectContaining({
      headers: expect.objectContaining({ "x-hito-account": props.user.id }),
      signal: expect.any(AbortSignal),
    }),
  ]);
  expect(router.reload).not.toHaveBeenCalled();
  await act(async () => {
    fireEvent.click(refresh);
  });
  const posted = fetch.mock.calls.find(([path]) => path === refreshPath);
  expect(posted).toBeDefined();
  expect(posted![1]).toMatchObject({
    method: "POST",
    credentials: "same-origin",
    headers: { "x-hito-account": props.user.id },
    signal: expect.any(AbortSignal),
  });
  const request: Schema["RefreshFollowedPlaylist"] = JSON.parse(
    String(posted![1]?.body),
  );
  expect(request).toEqual({
    request_id: expect.stringMatching(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    ),
  });
  expect((refresh as HTMLButtonElement).disabled).toBe(true);
  expect(rows().map((row) => row.textContent)).toEqual(retainedEntries);
  expect(router.reload).not.toHaveBeenCalled();
  await act(async () => {
    accepted.resolve(
      Response.json(
        {
          request_id: request.request_id,
          playlist_id: ready.playlist_id,
          job_id: "bd39a2fe-b459-4c5a-945c-6cfaa8b240fe",
        } satisfies Schema["PlaylistFollowRefresh"],
        { status: 202 },
      ),
    );
  });
  if (statusReads === 1)
    await act(async () => {
      await vi.advanceTimersToNextTimerAsync();
    });
  expect(statusReads).toBe(2);
  expect(updates.getByText(/Queued/)).toBeTruthy();
  expect(rows().map((row) => row.textContent)).toEqual(retainedEntries);
  expect(router.reload).not.toHaveBeenCalled();
  await act(async () => {
    await vi.advanceTimersToNextTimerAsync();
  });
  expect(statusReads).toBe(3);
  await act(async () => {
    running.resolve(
      Response.json({
        ...ready,
        status: "running",
      } satisfies Schema["PlaylistFollow"]),
    );
  });
  expect(updates.getByText(/Updating/)).toBeTruthy();
  expect(screen.getByRole("table", { name: "Playlist entries" })).toBe(table);
  expect(rows().map((row) => row.textContent)).toEqual(retainedEntries);
  expect(router.reload).not.toHaveBeenCalled();
  await act(async () => {
    await vi.advanceTimersToNextTimerAsync();
  });
  expect(statusReads).toBe(4);
  await act(async () => completed.resolve(Response.json(ready)));
  expect(updates.getByText("Ready", { exact: true })).toBeTruthy();
  expect(router.reload).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({ only: ["detail", "playlists"] }),
  );
  await act(async () => {
    await vi.advanceTimersByTimeAsync(5000);
  });
  expect(router.reload).toHaveBeenCalledTimes(1);
  expect(play).not.toHaveBeenCalled();
  expect(
    fetch.mock.calls.filter(([path]) => path === refreshPath),
  ).toHaveLength(1);
  const requestsBeforeUnmount = fetch.mock.calls.length;
  view.unmount();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(fetch).toHaveBeenCalledTimes(requestsBeforeUnmount);
});
