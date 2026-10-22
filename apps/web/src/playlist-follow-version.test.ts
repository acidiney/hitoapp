import { createElement, type ReactNode } from "react";
import { act, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import PlaylistPage from "./pages/PlaylistPage";
import { NoticeProvider } from "./shared/ui/Notices";
import { PlaybackProvider } from "./player/PlaybackProvider";
import { copiedPlaylistPage } from "./fixtures/playlistPages";
import type { components } from "./generated";
import type { PlaylistPageProps } from "./shared/types";

type Schema = components["schemas"];
type ReloadOptions = NonNullable<Parameters<typeof router.reload>[0]>;
type ReloadVisit = Parameters<NonNullable<ReloadOptions["onFinish"]>>[0];
type ReloadPage = Parameters<NonNullable<ReloadOptions["onSuccess"]>>[0];

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

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

function completedVisit(options: ReloadOptions, playlist: string): ReloadVisit {
  const noop = () => {};
  return {
    id: "accepted-version-detail-reload",
    url: new URL(`http://localhost/playlists/${playlist}`),
    method: "get",
    data: {},
    completed: true,
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

test("the first Follow content revision reconciles reused provider tokens without reloading for freshness alone", async () => {
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
      follow_content_revision: 1,
    },
  };
  const updatedPlaylist = {
    ...before.detail.playlist,
    name: "Selection after an automatic update",
  };
  const after: PlaylistPageProps = {
    ...before,
    playlists: [updatedPlaylist],
    detail: {
      ...before.detail,
      playlist: updatedPlaylist,
      follow_content_revision: 2,
      entries: [before.detail.entries[2], before.detail.entries[0]].map(
        (entry, position) => ({ ...entry, position }),
      ),
    },
  };
  const ready: Schema["PlaylistFollow"] = {
    playlist_id: before.detail.playlist.id,
    generation: 1,
    content_revision: 2,
    interval: "hourly",
    snapshot: "retained-snapshot",
    last_refreshed_at: Math.floor(Date.now() / 1000),
    next_refresh_at: Math.floor(Date.now() / 1000) + 3600,
    status: "ready",
    error: null,
  };
  expect(before.detail.origin!.snapshot).toBe(ready.snapshot);
  expect(after.detail.origin!.snapshot).toBe(ready.snapshot);
  const statusPath = `/api/v1/playlists/${ready.playlist_id}/follow`;
  let resolveInitial!: (response: Response) => void;
  const initialResponse = new Promise<Response>((resolve) => {
    resolveInitial = resolve;
  });
  let currentStatus = ready;
  let statusReads = 0;
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    if (path !== statusPath || (options?.method ?? "GET") !== "GET")
      throw new Error("Content version reconciliation must only read status");
    statusReads++;
    return statusReads === 1
      ? initialResponse
      : Promise.resolve(Response.json(currentStatus));
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(page(before));
  const table = screen.getByRole("table", { name: "Playlist entries" });
  const oldEntries = within(table)
    .getAllByRole("row")
    .slice(1)
    .map((row) => row.textContent);
  expect(router.reload).not.toHaveBeenCalled();
  await act(async () => resolveInitial(Response.json(ready)));
  expect(statusReads).toBe(1);
  expect(fetch.mock.calls[0][1]).toMatchObject({
    credentials: "same-origin",
    headers: { "x-hito-account": before.user.id },
    signal: expect.any(AbortSignal),
  });
  expect(screen.getByText("Ready", { exact: true })).toBeTruthy();
  expect(
    within(table)
      .getAllByRole("row")
      .slice(1)
      .map((row) => row.textContent),
  ).toEqual(oldEntries);
  expect(
    screen.getByRole("heading", { name: before.detail.playlist.name }),
  ).toBeTruthy();
  expect(router.reload).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({ only: ["detail", "playlists"] }),
  );
  const reload = vi.mocked(router.reload).mock.calls[0][0];
  if (!reload) throw new Error("The content update must request detail");
  const accepted: ReloadPage = {
    component: "PlaylistPage",
    url: `/playlists/${ready.playlist_id}`,
    version: null,
    props: { ...after, errors: {} },
    rescuedProps: [],
    flash: {},
    rememberedState: {},
  };
  view.rerender(page(after));
  await act(async () => {
    await reload.onSuccess?.(accepted);
    reload.onFinish?.(completedVisit(reload, ready.playlist_id));
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
  expect(
    within(updatedRows[1]).getByRole("button", { name: "Play Fixture A" }),
  ).toBeTruthy();
  currentStatus = {
    ...ready,
    last_refreshed_at: ready.last_refreshed_at + 30,
    next_refresh_at: ready.next_refresh_at + 30,
  };
  await act(async () => {
    await vi.advanceTimersByTimeAsync(30_000);
  });
  expect(statusReads).toBe(2);
  expect(router.reload).toHaveBeenCalledTimes(1);
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
  });
  expect(statusReads).toBe(3);
  expect(router.reload).toHaveBeenCalledTimes(1);
  expect(router.visit).not.toHaveBeenCalled();
  expect(play).not.toHaveBeenCalled();
  const beforeUnmount = fetch.mock.calls.length;
  view.unmount();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(fetch).toHaveBeenCalledTimes(beforeUnmount);
});
