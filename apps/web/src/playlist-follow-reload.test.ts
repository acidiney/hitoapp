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
type ReloadOptions = NonNullable<Parameters<typeof router.reload>[0]>;
type ReloadVisit = Parameters<NonNullable<ReloadOptions["onFinish"]>>[0];
type ReloadPage = Parameters<NonNullable<ReloadOptions["onSuccess"]>>[0];

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

function reloadVisit(options: ReloadOptions, completed: boolean): ReloadVisit {
  const noop = () => {};
  return {
    id: completed ? "accepted-detail-reload" : "failed-detail-reload",
    url: new URL(
      "http://localhost/playlists/eb0f0af1-dc27-4737-ad8a-928b8bf6a32e",
    ),
    method: "get",
    data: {},
    completed,
    cancelled: false,
    interrupted: false,
    replace: false,
    preserveScroll: true,
    preserveState: true,
    only: options.only ?? [],
    except: [],
    headers: {},
    errorBag: null,
    forceFormData: false,
    queryStringArrayFormat: "brackets",
    async: true,
    showProgress: false,
    prefetch: false,
    fresh: true,
    reset: [],
    preserveUrl: true,
    preserveErrors: false,
    invalidateCacheTags: [],
    viewTransition: false,
    component: null,
    pageProps: null,
    cached: false,
    onCancelToken: options.onCancelToken ?? noop,
    onBefore: options.onBefore ?? noop,
    onBeforeUpdate: options.onBeforeUpdate ?? noop,
    onStart: options.onStart ?? noop,
    onProgress: options.onProgress ?? noop,
    onFinish: options.onFinish ?? noop,
    onCancel: options.onCancel ?? noop,
    onSuccess: options.onSuccess ?? noop,
    onError: options.onError ?? noop,
    onHttpException: options.onHttpException ?? noop,
    onNetworkError: options.onNetworkError ?? noop,
    onFlash: options.onFlash ?? noop,
    onPrefetched: options.onPrefetched ?? noop,
    onPrefetching: options.onPrefetching ?? noop,
  };
}

test("a failed detail reload retries completed Follow content until updated props are accepted", async () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-10-17T03:00:00Z"));
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const before: PlaylistPageProps = {
    ...copiedPlaylistPage,
    detail: {
      ...copiedPlaylistPage.detail,
      playlist: { ...copiedPlaylistPage.detail.playlist, kind: "followed" },
    },
  };
  const updatedPlaylist = {
    ...before.detail.playlist,
    name: "Selection after recovered refresh",
  };
  const after: PlaylistPageProps = {
    ...before,
    playlists: [updatedPlaylist],
    detail: {
      ...before.detail,
      playlist: updatedPlaylist,
      entries: [before.detail.entries[2], before.detail.entries[0]].map(
        (entry, position) => ({ ...entry, position }),
      ),
    },
  };
  const ready: Schema["PlaylistFollow"] = {
    playlist_id: before.detail.playlist.id,
    interval: "hourly",
    snapshot: "retained-snapshot",
    last_refreshed_at: Math.floor(Date.now() / 1000),
    next_refresh_at: Math.floor(Date.now() / 1000) + 3600,
    status: "ready",
    error: null,
  };
  const statusPath = `/api/v1/playlists/${ready.playlist_id}/follow`;
  const refreshPath = `${statusPath}/refresh`;
  let statusReads = 0;
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    if (path === statusPath && (options?.method ?? "GET") === "GET") {
      statusReads++;
      return Promise.resolve(Response.json(ready));
    }
    if (path === refreshPath && options?.method === "POST") {
      const request: Schema["RefreshFollowedPlaylist"] = JSON.parse(
        String(options.body),
      );
      return Promise.resolve(
        Response.json(
          {
            request_id: request.request_id,
            playlist_id: ready.playlist_id,
            job_id: "f6014caa-e942-4cea-87ce-f3107133b843",
          } satisfies Schema["PlaylistFollowRefresh"],
          { status: 202 },
        ),
      );
    }
    throw new Error("Unexpected request outside completed detail recovery");
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(page(before));
  await act(async () => {});
  expect(statusReads).toBe(1);
  expect(router.reload).not.toHaveBeenCalled();
  const table = screen.getByRole("table", { name: "Playlist entries" });
  const oldEntries = within(table)
    .getAllByRole("row")
    .slice(1)
    .map((row) => row.textContent);
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Refresh now" }));
  });
  expect(statusReads).toBe(2);
  expect(router.reload).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({ only: ["detail", "playlists"] }),
  );
  const failed = vi.mocked(router.reload).mock.calls[0][0];
  if (!failed) throw new Error("A detail reload must include its options");
  await act(async () => {
    failed.onCancelToken?.({ cancel: vi.fn() });
    failed.onStart?.(reloadVisit(failed, false));
    failed.onNetworkError?.(
      new Error("The detail request lost its connection"),
    );
    failed.onFinish?.(reloadVisit(failed, false));
  });
  expect(
    within(table)
      .getAllByRole("row")
      .slice(1)
      .map((row) => row.textContent),
  ).toEqual(oldEntries);
  expect(
    screen.getByRole("heading", { name: before.detail.playlist.name }),
  ).toBeTruthy();
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
  });
  expect(statusReads).toBe(3);
  expect(router.reload).toHaveBeenCalledTimes(2);
  expect(vi.mocked(router.reload).mock.calls[1][0]).toMatchObject({
    only: ["detail", "playlists"],
  });
  const recovered = vi.mocked(router.reload).mock.calls[1][0];
  if (!recovered)
    throw new Error("A retried detail reload must include options");
  const accepted: ReloadPage = {
    component: "PlaylistPage",
    url: `/playlists/${ready.playlist_id}`,
    version: null,
    props: { ...after, errors: {} },
    rescuedProps: [],
    flash: {},
    rememberedState: {},
  };
  await act(async () => {
    recovered.onCancelToken?.({ cancel: vi.fn() });
    recovered.onStart?.(reloadVisit(recovered, false));
  });
  view.rerender(page(after));
  await act(async () => {
    await recovered.onSuccess?.(accepted);
    recovered.onFinish?.(reloadVisit(recovered, true));
  });
  expect(
    screen.getByRole("heading", { name: after.detail.playlist.name }),
  ).toBeTruthy();
  const updatedRows = within(
    screen.getByRole("table", { name: "Playlist entries" }),
  )
    .getAllByRole("row")
    .slice(1);
  expect(updatedRows).toHaveLength(2);
  expect(
    within(updatedRows[0]).getByRole("button", { name: "Play Fixture B" }),
  ).toBeTruthy();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(30_000);
    window.dispatchEvent(new Event("focus"));
  });
  expect(router.reload).toHaveBeenCalledTimes(2);
  expect(
    fetch.mock.calls.filter(([path]) => path === refreshPath),
  ).toHaveLength(1);
  expect(router.visit).not.toHaveBeenCalled();
  expect(play).not.toHaveBeenCalled();
  const beforeUnmount = fetch.mock.calls.length;
  view.unmount();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(fetch).toHaveBeenCalledTimes(beforeUnmount);
});
