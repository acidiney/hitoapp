import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
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

function heldResponse() {
  let resolve!: (response: Response) => void;
  const response = new Promise<Response>((accept) => {
    resolve = accept;
  });
  return { response, resolve };
}

function page(props: PlaylistPageProps) {
  return createElement(
    NoticeProvider,
    null,
    createElement(PlaybackProvider, {
      account: props.user.id,
      children: createElement(PlaylistPage, props),
    }),
  );
}

test("account and playlist changes fence held Follow responses and keep polling with the current owner", async () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-10-17T03:00:00Z"));
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const first: PlaylistPageProps = {
    ...copiedPlaylistPage,
    detail: {
      ...copiedPlaylistPage.detail,
      playlist: { ...copiedPlaylistPage.detail.playlist, kind: "followed" },
      follow_content_revision: 1,
    },
  };
  const second: PlaylistPageProps = {
    ...first,
    user: { id: "bob", username: "Bob", admin: false },
    detail: {
      ...first.detail,
      playlist: {
        id: "88fefb77-e103-4c73-8e6b-bdf6ee867184",
        name: "Bob's first selection",
        kind: "followed",
      },
    },
  };
  const current: PlaylistPageProps = {
    ...second,
    detail: {
      ...second.detail,
      playlist: {
        id: "4aeebd10-09bf-4988-9012-705dbd78c1d2",
        name: "Bob's current selection",
        kind: "followed",
      },
      entries: second.detail.entries.slice(0, 2),
    },
  };
  const ready = (props: PlaylistPageProps): Schema["PlaylistFollow"] => {
    const interval = props.user.id === first.user.id ? "hourly" : "daily";
    return {
      playlist_id: props.detail.playlist.id,
      generation: 1,
      content_revision: 1,
      interval,
      snapshot: "retained-snapshot",
      last_refreshed_at: Math.floor(Date.now() / 1000),
      next_refresh_at:
        Math.floor(Date.now() / 1000) + (interval === "hourly" ? 3600 : 86400),
      status: "ready",
      error: null,
    };
  };
  const firstReady = ready(first);
  const secondReady = ready(second);
  const currentReady = ready(current);
  const statusPath = (props: PlaylistPageProps) =>
    `/api/v1/playlists/${props.detail.playlist.id}/follow`;
  const downloadPath = (props: PlaylistPageProps) =>
    `/api/v1/playlists/${props.detail.playlist.id}/download`;
  const owners = [first, second, current];
  const accountOf = (options?: RequestInit) =>
    new Headers(options?.headers).get("x-hito-account");
  const firstPath = statusPath(first);
  const secondPath = statusPath(second);
  const currentPath = statusPath(current);
  const secondRefreshPath = `${secondPath}/refresh`;
  const oldRead = heldResponse();
  const oldWrite = heldResponse();
  const currentRead = heldResponse();
  let firstReads = 0;
  let currentReads = 0;
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (
      method === "GET" &&
      owners.some(
        (props) =>
          path === downloadPath(props) && accountOf(options) === props.user.id,
      )
    )
      return Promise.resolve(
        Response.json({ error: "Not found" }, { status: 404 }),
      );
    if (path === firstPath && method === "GET") {
      firstReads++;
      return firstReads === 1
        ? Promise.resolve(Response.json(firstReady))
        : oldRead.response;
    }
    if (path === secondPath && method === "GET")
      return Promise.resolve(Response.json(secondReady));
    if (path === secondRefreshPath && method === "POST")
      return oldWrite.response;
    if (path === currentPath && method === "GET") {
      currentReads++;
      return currentReads === 2
        ? currentRead.response
        : Promise.resolve(Response.json(currentReady));
    }
    throw new Error("Unexpected request outside Follow ownership checks");
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(page(first));
  await act(async () => {});
  expect(screen.getByText("Every hour", { exact: true })).toBeTruthy();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(30_000);
  });
  expect(firstReads).toBe(2);
  const firstRequests = fetch.mock.calls.filter(([path]) => path === firstPath);
  const oldReadSignal = firstRequests[1][1]?.signal;
  expect(oldReadSignal?.aborted).toBe(false);
  expect(firstRequests[1][1]?.headers).toMatchObject({
    "x-hito-account": first.user.id,
  });
  view.rerender(page(second));
  await act(async () => {});
  expect(oldReadSignal?.aborted).toBe(true);
  expect(
    screen.getByRole("heading", { name: second.detail.playlist.name }),
  ).toBeTruthy();
  expect(screen.getByText("Every day", { exact: true })).toBeTruthy();
  const secondTable = screen.getByRole("table", { name: "Playlist entries" });
  const secondEntries = within(secondTable).getAllByRole("row").slice(1);
  expect(secondEntries).toHaveLength(second.detail.entries.length);
  const refresh = screen.getByRole("button", { name: "Refresh now" });
  await act(async () => {
    fireEvent.click(refresh);
  });
  const posted = fetch.mock.calls.find(([path]) => path === secondRefreshPath);
  expect(posted).toBeDefined();
  const oldWriteSignal = posted![1]?.signal;
  expect(oldWriteSignal?.aborted).toBe(false);
  expect(posted![1]?.headers).toMatchObject({
    "x-hito-account": second.user.id,
  });
  const request: Schema["RefreshFollowedPlaylist"] = JSON.parse(
    String(posted![1]?.body),
  );
  await act(async () => {
    oldRead.resolve(
      Response.json({
        ...firstReady,
        status: "failed",
        error: "refresh_failed",
      } satisfies Schema["PlaylistFollow"]),
    );
  });
  expect(screen.getByText("Every day", { exact: true })).toBeTruthy();
  expect(screen.queryByText("Update failed", { exact: true })).toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();
  expect((refresh as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getByRole("table", { name: "Playlist entries" })).toBe(
    secondTable,
  );
  expect(router.reload).not.toHaveBeenCalled();
  view.rerender(page(current));
  await act(async () => {});
  expect(oldWriteSignal?.aborted).toBe(true);
  const table = screen.getByRole("table", { name: "Playlist entries" });
  const entries = () =>
    within(table)
      .getAllByRole("row")
      .slice(1)
      .map((row) => row.textContent);
  const currentEntries = entries();
  expect(currentEntries).toHaveLength(current.detail.entries.length);
  expect(
    screen.getByRole("heading", { name: current.detail.playlist.name }),
  ).toBeTruthy();
  expect(
    screen.queryByRole("button", { name: "Retry refresh request" }),
  ).toBeNull();
  expect(
    (screen.getByRole("button", { name: "Refresh now" }) as HTMLButtonElement)
      .disabled,
  ).toBe(false);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(30_000);
  });
  expect(currentReads).toBe(2);
  const currentRequests = () =>
    fetch.mock.calls.filter(([path]) => path === currentPath);
  const currentSignal = currentRequests()[1][1]?.signal;
  expect(currentSignal?.aborted).toBe(false);
  await act(async () => {
    oldWrite.resolve(
      Response.json(
        {
          request_id: request.request_id,
          playlist_id: second.detail.playlist.id,
          job_id: "24bb3ab0-f41d-43e1-9828-87a1d35f2bbc",
        } satisfies Schema["PlaylistFollowRefresh"],
        { status: 202 },
      ),
    );
  });
  expect(currentSignal?.aborted).toBe(false);
  expect(screen.getByText("Ready", { exact: true })).toBeTruthy();
  expect(screen.getByText("Every day", { exact: true })).toBeTruthy();
  expect(entries()).toEqual(currentEntries);
  expect(screen.queryByRole("alert")).toBeNull();
  expect(router.reload).not.toHaveBeenCalled();
  await act(async () => currentRead.resolve(Response.json(currentReady)));
  const beforePoll = fetch.mock.calls.length;
  await act(async () => {
    await vi.advanceTimersByTimeAsync(30_000);
  });
  expect(fetch.mock.calls.slice(beforePoll)).toEqual([
    [
      currentPath,
      expect.objectContaining({
        headers: expect.objectContaining({
          "x-hito-account": current.user.id,
        }),
        signal: expect.any(AbortSignal),
      }),
    ],
  ]);
  expect(firstReads).toBe(2);
  expect(fetch.mock.calls.filter(([path]) => path === secondPath)).toHaveLength(
    1,
  );
  expect(currentReads).toBe(3);
  for (const props of owners) {
    expect(
      fetch.mock.calls.filter(
        ([path, options]) =>
          path === downloadPath(props) && accountOf(options) === props.user.id,
      ),
    ).toEqual([
      [
        downloadPath(props),
        expect.objectContaining({
          method: "GET",
          credentials: "same-origin",
          headers: expect.objectContaining({ "x-hito-account": props.user.id }),
          signal: expect.any(AbortSignal),
        }),
      ],
    ]);
  }
  expect(entries()).toEqual(currentEntries);
  expect(router.reload).not.toHaveBeenCalled();
  expect(router.visit).not.toHaveBeenCalled();
  expect(play).not.toHaveBeenCalled();
  const beforeUnmount = fetch.mock.calls.length;
  view.unmount();
  expect(currentSignal?.aborted).toBe(true);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
    window.dispatchEvent(new Event("focus"));
  });
  expect(fetch).toHaveBeenCalledTimes(beforeUnmount);
});
