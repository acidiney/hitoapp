import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import { act, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import PlaylistPage from "../pages/PlaylistPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import { copiedPlaylistPage } from "../fixtures/playlistPages";
import type { components } from "../../../generated";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

function page(props: Schema<"PlaylistPageProps">) {
  return createElement(
    NoticeProvider,
    null,
    createElement(PlaybackProvider, {
      account: props.user.id,
      children: createElement(PlaylistPage, props),
    }),
  );
}

test("a newer first Follow status reloads stale server-rendered entries and applies the accepted playlist props", async () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-10-17T03:00:00Z"));
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const origin = copiedPlaylistPage.detail.origin;
  if (!origin) throw new Error("The playlist fixture must have its origin");
  const before: Schema<"PlaylistPageProps"> = {
    ...copiedPlaylistPage,
    detail: {
      ...copiedPlaylistPage.detail,
      playlist: { ...copiedPlaylistPage.detail.playlist, kind: "followed" },
      origin: { ...origin, snapshot: "provider-snapshot-before-status" },
      follow_content_revision: 1,
    },
  };
  const updatedPlaylist = {
    ...before.detail.playlist,
    name: "Provider selection after update",
  };
  const after: Schema<"PlaylistPageProps"> = {
    ...before,
    playlists: [updatedPlaylist],
    detail: {
      ...before.detail,
      playlist: updatedPlaylist,
      follow_content_revision: 2,
      origin: {
        ...origin,
        snapshot: "provider-snapshot-after-status",
        attribution: { label: "Updated provider attribution" },
      },
      entries: [before.detail.entries[2], before.detail.entries[0]].map(
        (entry, position) => ({ ...entry, position }),
      ),
    },
  };
  const ready: Schema<"PlaylistFollow"> = {
    playlist_id: before.detail.playlist.id,
    generation: 1,
    content_revision: 2,
    interval: "hourly",
    snapshot: after.detail.origin!.snapshot,
    last_refreshed_at: Math.floor(Date.now() / 1000),
    next_refresh_at: Math.floor(Date.now() / 1000) + 3600,
    status: "ready",
    error: null,
  };
  expect(before.detail.origin!.snapshot).not.toBe(ready.snapshot);
  const statusPath = `/api/v1/playlists/${ready.playlist_id}/follow`;
  let resolveInitial!: (response: Response) => void;
  const initialResponse = new Promise<Response>((resolve) => {
    resolveInitial = resolve;
  });
  let statusReads = 0;
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    if (path !== statusPath || (options?.method ?? "GET") !== "GET")
      throw new Error("Content reconciliation must only read Follow status");
    statusReads++;
    return statusReads === 1
      ? initialResponse
      : Promise.resolve(Response.json(ready));
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(page(before));
  const table = screen.getByRole("table", { name: "Playlist entries" });
  const rows = () => within(table).getAllByRole("row").slice(1);
  const oldEntries = rows().map((row) => row.textContent);
  expect(oldEntries).toHaveLength(before.detail.entries.length);
  expect(
    screen.getByRole("heading", { name: before.detail.playlist.name }),
  ).toBeTruthy();
  expect(router.reload).not.toHaveBeenCalled();
  await act(async () => resolveInitial(Response.json(ready)));
  expect(statusReads).toBe(1);
  expect(fetch.mock.calls[0][1]).toMatchObject({
    credentials: "same-origin",
    headers: { "x-hito-account": before.user.id },
    signal: expect.any(AbortSignal),
  });
  expect(screen.getByText("Ready", { exact: true })).toBeTruthy();
  expect(rows().map((row) => row.textContent)).toEqual(oldEntries);
  expect(
    screen.getByRole("heading", { name: before.detail.playlist.name }),
  ).toBeTruthy();
  expect(router.reload).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({ only: ["detail", "playlists"] }),
  );
  view.rerender(page(after));
  await act(async () => {});
  expect(
    screen.getByRole("heading", { name: after.detail.playlist.name }),
  ).toBeTruthy();
  expect(
    screen.queryByRole("heading", { name: before.detail.playlist.name }),
  ).toBeNull();
  expect(screen.getByText("Updated provider attribution")).toBeTruthy();
  expect(screen.queryByText("Curated by the fixture owner")).toBeNull();
  const updatedTable = screen.getByRole("table", { name: "Playlist entries" });
  const updatedRows = within(updatedTable).getAllByRole("row").slice(1);
  expect(updatedRows).toHaveLength(2);
  expect(
    within(updatedRows[0]).getByRole("button", { name: "Play Fixture B" }),
  ).toBeTruthy();
  expect(
    within(updatedRows[1]).getByRole("button", { name: "Play Fixture A" }),
  ).toBeTruthy();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(30_000);
  });
  expect(statusReads).toBeGreaterThan(1);
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
