import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import PlaylistPage from "../pages/PlaylistPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import { followedPage, pausedFollow } from "../fixtures/pausedFollow";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

test("a definite Resume conflict reloads the current review and a new operation carries the current approval pins", async () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-10-17T03:00:00Z"));
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const logs = (["debug", "info", "warn", "error"] as const).map((level) =>
    vi.spyOn(console, level).mockImplementation(() => {}),
  );
  const props = followedPage();
  const stale = pausedFollow(props.detail.playlist.id, 3);
  const fresh = pausedFollow(props.detail.playlist.id, 5, {
    installation_revision: 9,
    service_revision: 6,
  });
  const rawConflict =
    "Stale approval at https://private-provider.invalid/?secret=private-value";
  let currentStatus = stale;
  let statusReads = 0;
  let syncRequests = 0;
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (path === `/api/v1/playlists/${stale.playlist_id}/follow`) {
      statusReads++;
      return Promise.resolve(Response.json(currentStatus));
    }
    if (path === "/api/v1/sync" && method === "POST") {
      syncRequests++;
      if (syncRequests === 1) {
        currentStatus = fresh;
        return Promise.resolve(
          Response.json({ error: rawConflict }, { status: 409 }),
        );
      }
      currentStatus = { ...fresh, generation: 6, status: "ready", error: null };
      return Promise.resolve(
        Response.json({
          acknowledged: [JSON.parse(String(options?.body)).operations[0].id],
        } satisfies Schema<"SyncReceipt">),
      );
    }
    throw new Error("Unexpected request outside conflict approval recovery");
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
  const entries = () =>
    within(screen.getByRole("table", { name: "Playlist entries" }))
      .getAllByRole("row")
      .slice(1)
      .map((row) => row.textContent);
  const savedEntries = entries();
  expect(statusReads).toBe(1);
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Resume updates" }));
  });
  const posts = () =>
    fetch.mock.calls.filter(([path]) => path === "/api/v1/sync");
  const first: Schema<"SyncBatch"> = JSON.parse(String(posts()[0][1]?.body));
  expect(first.operations[0].action).toEqual({
    kind: "playlist_follow_resume",
    playlist_id: stale.playlist_id,
    expected_generation: 3,
    installation_revision: 8,
    service_revision: 5,
  } satisfies Schema<"Action">);
  expect(statusReads).toBe(2);
  const approval = within(
    screen.getByRole("region", { name: "Resume playlist updates" }),
  );
  expect(approval.getByRole("alert").textContent).toMatch(
    /changed\. Check what changed/,
  );
  expect(approval.getByRole("alert").textContent).not.toContain(rawConflict);
  expect(
    screen.queryByRole("button", { name: "Retry resume request" }),
  ).toBeNull();
  expect(entries()).toEqual(savedEntries);
  await act(async () => {
    fireEvent.click(approval.getByRole("button", { name: "Resume updates" }));
  });
  expect(posts()).toHaveLength(2);
  const second: Schema<"SyncBatch"> = JSON.parse(String(posts()[1][1]?.body));
  expect(second.device_id).toBe(first.device_id);
  expect(second.operations[0].id).not.toBe(first.operations[0].id);
  expect(second.operations[0].action).toEqual({
    kind: "playlist_follow_resume",
    playlist_id: stale.playlist_id,
    expected_generation: 5,
    installation_revision: 9,
    service_revision: 6,
  } satisfies Schema<"Action">);
  expect(statusReads).toBe(3);
  expect(screen.getByText("Ready", { exact: true })).toBeTruthy();
  expect(
    screen.queryByRole("region", { name: "Resume playlist updates" }),
  ).toBeNull();
  expect(entries()).toEqual(savedEntries);
  expect(router.reload).not.toHaveBeenCalled();
  expect(router.visit).not.toHaveBeenCalled();
  expect(play).not.toHaveBeenCalled();
  const captured = JSON.stringify(logs.flatMap((sink) => sink.mock.calls));
  for (const privateValue of [
    rawConflict,
    stale.snapshot,
    props.detail.playlist.name,
  ])
    expect(captured).not.toContain(privateValue);
  const beforeUnmount = fetch.mock.calls.length;
  view.unmount();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(fetch).toHaveBeenCalledTimes(beforeUnmount);
});
