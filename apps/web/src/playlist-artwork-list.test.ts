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
import { ArtworkProvider } from "./ArtworkProvider";
import { NoticeProvider } from "./components/Notices";
import PlaylistIndexPage from "./pages/PlaylistIndexPage";
import { copiedPlaylistPage } from "./fixtures/playlistPages";
import { playlistImportPage } from "./fixtures/playlistImports";
import type { Playlist, PlaylistArtworkFetch } from "./types";

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
});

test("playlist rows retrieve imported covers by playlist identity and keep unsupported rows as icons", async () => {
  vi.useFakeTimers();
  const copied: Playlist = {
    ...copiedPlaylistPage.detail.playlist,
    name: "My renamed copy",
  };
  const manual: Playlist = {
    id: "manual-selection",
    name: "My local playlist",
    kind: "local",
  };
  const followed: Playlist = {
    id: "followed-selection",
    name: "My followed playlist",
    kind: "followed",
  };
  const image = `/api/v1/playlists/${copied.id}/artwork`;
  const manualImage = `/api/v1/playlists/${manual.id}/artwork`;
  const ready: PlaylistArtworkFetch = {
    id: "retained-playlist-cover",
    playlist_id: copied.id,
    status: "ready",
    attempts: 1,
    error: null,
    created_at: 1,
    updated_at: 2,
  };
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    if (options?.method === "GET" && path === `${image}/availability`)
      return Promise.resolve(Response.json({ can_fetch: true }));
    if (options?.method === "GET" && path === `${manualImage}/availability`)
      return Promise.resolve(Response.json({ can_fetch: false }));
    if (options?.method === "POST" && path === `${image}/fetch`)
      return Promise.resolve(Response.json(ready, { status: 202 }));
    throw new Error("Unexpected request outside imported playlist artwork");
  });
  vi.stubGlobal("fetch", fetch);
  const { unmount } = render(
    createElement(NoticeProvider, {
      children: createElement(ArtworkProvider, {
        account: playlistImportPage.user.id,
        children: createElement(PlaylistIndexPage, {
          ...playlistImportPage,
          playlists: [copied, manual, followed],
        }),
      }),
    }),
  );
  const copyRow = screen.getByRole("link", { name: /My renamed copy/ });
  const manualRow = screen.getByRole("link", { name: /My local playlist/ });
  const followedRow = screen.getByRole("link", {
    name: /My followed playlist/,
  });
  const cover = (row: HTMLElement) => row.querySelector(".artwork img");
  expect(cover(copyRow)?.getAttribute("src")).toBe(image);
  expect(cover(manualRow)?.getAttribute("src")).toBe(manualImage);
  expect(cover(followedRow)).toBeNull();
  expect(fetch).not.toHaveBeenCalled();
  await act(async () => {
    fireEvent.error(cover(copyRow)!);
  });
  expect(cover(copyRow)?.getAttribute("src")).toBe(image);
  expect(within(copyRow).getByText("My renamed copy")).toBeTruthy();
  await act(async () => {
    fireEvent.error(cover(manualRow)!);
  });
  expect(cover(manualRow)).toBeNull();
  expect(manualRow.querySelector(".artwork svg")).toBeTruthy();
  expect(followedRow.querySelector(".artwork svg")).toBeTruthy();
  expect(fetch.mock.calls.map(([path]) => path)).toEqual([
    `${image}/availability`,
    `${image}/fetch`,
    `${manualImage}/availability`,
  ]);
  for (const [, options] of fetch.mock.calls) {
    expect(options?.credentials).toBe("same-origin");
    expect(options?.headers).toEqual(
      expect.objectContaining({ "x-hito-account": playlistImportPage.user.id }),
    );
    expect(options?.signal).toBeInstanceOf(AbortSignal);
    expect(options?.body).toBeUndefined();
  }
  unmount();
  expect(vi.getTimerCount()).toBe(0);
});
