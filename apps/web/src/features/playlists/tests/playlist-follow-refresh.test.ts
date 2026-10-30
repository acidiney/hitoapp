import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
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

test("a lost refresh retries its original request and reloads once when the first check is already unchanged Ready", async () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-10-17T03:00:00Z"));
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const logs = (["debug", "info", "warn", "error"] as const).map((level) =>
    vi.spyOn(console, level).mockImplementation(() => {}),
  );
  const props: Schema<"PlaylistPageProps"> = {
    ...copiedPlaylistPage,
    detail: {
      ...copiedPlaylistPage.detail,
      playlist: { ...copiedPlaylistPage.detail.playlist, kind: "followed" },
      follow_content_revision: 1,
    },
  };
  const ready: Schema<"PlaylistFollow"> = {
    playlist_id: props.detail.playlist.id,
    generation: 1,
    content_revision: 1,
    interval: "hourly",
    snapshot: "retained-snapshot",
    last_refreshed_at: Math.floor(Date.now() / 1000),
    next_refresh_at: Math.floor(Date.now() / 1000) + 3600,
    status: "ready",
    error: null,
  };
  const statusPath = `/api/v1/playlists/${ready.playlist_id}/follow`;
  const refreshPath = `${statusPath}/refresh`;
  const rawError =
    "Connection lost at https://provider.invalid/private-playlist?signed=private-secret";
  let resolveRetry!: (response: Response) => void;
  const retryResponse = new Promise<Response>((resolve) => {
    resolveRetry = resolve;
  });
  let statusReads = 0;
  let refreshes = 0;
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    if (path === statusPath && (options?.method ?? "GET") === "GET") {
      statusReads++;
      return Promise.resolve(Response.json(ready));
    }
    if (path === refreshPath && options?.method === "POST") {
      refreshes++;
      if (refreshes === 1) return Promise.reject(new TypeError(rawError));
      if (refreshes === 2) return retryResponse;
    }
    throw new Error("Unexpected request outside playlist refresh recovery");
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
  await act(async () => {});
  const region = screen.getByRole("region", { name: "Playlist updates" });
  const updates = within(region);
  expect(updates.getByText("Ready", { exact: true })).toBeTruthy();
  expect(statusReads).toBe(1);
  const table = screen.getByRole("table", { name: "Playlist entries" });
  const entries = () =>
    within(table)
      .getAllByRole("row")
      .slice(1)
      .map((row) => row.textContent);
  const originalEntries = entries();
  const refresh = updates.getByRole("button", { name: "Refresh now" });
  await act(async () => {
    fireEvent.click(refresh);
  });
  expect(refreshes).toBe(1);
  const posts = () => fetch.mock.calls.filter(([path]) => path === refreshPath);
  const originalBody = String(posts()[0][1]?.body);
  const request: Schema<"RefreshFollowedPlaylist"> = JSON.parse(originalBody);
  expect(request).toEqual({
    request_id: expect.stringMatching(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    ),
  });
  expect(updates.getByRole("alert").textContent).toMatch(
    /Retry the same request/,
  );
  expect(region.textContent).not.toContain(rawError);
  expect((refresh as HTMLButtonElement).disabled).toBe(true);
  expect(entries()).toEqual(originalEntries);
  expect(router.reload).not.toHaveBeenCalled();
  const retry = updates.getByRole("button", { name: "Retry refresh request" });
  await act(async () => {
    fireEvent.click(retry);
    fireEvent.click(retry);
  });
  expect(posts()).toHaveLength(2);
  for (const [, options] of posts())
    expect(options).toMatchObject({
      method: "POST",
      credentials: "same-origin",
      headers: { "x-hito-account": props.user.id },
      body: originalBody,
      signal: expect.any(AbortSignal),
    });
  expect(posts()[1][1]?.signal).not.toBe(posts()[0][1]?.signal);
  expect(posts()[1][1]?.signal?.aborted).toBe(false);
  expect((retry as HTMLButtonElement).disabled).toBe(true);
  expect(entries()).toEqual(originalEntries);
  expect(router.reload).not.toHaveBeenCalled();
  await act(async () => {
    resolveRetry(
      Response.json(
        {
          request_id: request.request_id,
          playlist_id: ready.playlist_id,
          job_id: "bca57b45-02b4-4786-aac5-e69d14b06843",
        } satisfies Schema<"PlaylistFollowRefresh">,
        { status: 202 },
      ),
    );
  });
  if (statusReads === 1)
    await act(async () => {
      await vi.advanceTimersToNextTimerAsync();
    });
  expect(statusReads).toBe(2);
  expect(updates.getByText("Ready", { exact: true })).toBeTruthy();
  expect(updates.queryByText(/Queued|Updating/)).toBeNull();
  expect(updates.queryByRole("alert")).toBeNull();
  expect(
    updates.queryByRole("button", { name: "Retry refresh request" }),
  ).toBeNull();
  expect(entries()).toEqual(originalEntries);
  expect(router.reload).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({ only: ["detail", "playlists"] }),
  );
  await act(async () => {
    await vi.advanceTimersByTimeAsync(5000);
  });
  expect(router.reload).toHaveBeenCalledTimes(1);
  expect(posts()).toHaveLength(2);
  expect(play).not.toHaveBeenCalled();
  const captured = JSON.stringify(logs.flatMap((sink) => sink.mock.calls));
  for (const privateValue of [
    rawError,
    ready.snapshot,
    props.detail.playlist.name,
    props.detail.origin!.attribution.label,
    ...props.detail.entries.map((entry) => entry.title),
  ])
    expect(captured).not.toContain(privateValue);
  const requestsBeforeUnmount = fetch.mock.calls.length;
  view.unmount();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(fetch).toHaveBeenCalledTimes(requestsBeforeUnmount);
});
