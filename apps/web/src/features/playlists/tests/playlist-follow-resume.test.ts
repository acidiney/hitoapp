import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import PlaylistPage from "../pages/PlaylistPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import { followedPage, pausedFollow } from "../fixtures/pausedFollow";
import { heldResponse } from "../../../test/async";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

test("a paused playlist explains the change, resumes with one action and retries an unconfirmed resume unchanged", async () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-10-17T03:00:00Z"));
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const logs = (["debug", "info", "warn", "error"] as const).map((level) =>
    vi.spyOn(console, level).mockImplementation(() => {}),
  );
  const props = followedPage();
  const paused = pausedFollow(props.detail.playlist.id, 3);
  const resumed: Schema<"PlaylistFollow"> = {
    ...paused,
    generation: 4,
    status: "ready",
    error: null,
    review: null,
  };
  const statusPath = `/api/v1/playlists/${paused.playlist_id}/follow`;
  const retryResponse = heldResponse();
  const rawError =
    "Lost response from https://private-provider.invalid/?token=private-secret";
  let currentStatus = paused;
  let statusReads = 0;
  let syncRequests = 0;
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (path === statusPath && method === "GET") {
      statusReads++;
      return Promise.resolve(Response.json(currentStatus));
    }
    if (path === "/api/v1/sync" && method === "POST") {
      syncRequests++;
      if (syncRequests === 1) {
        currentStatus = resumed;
        return Promise.reject(new TypeError(rawError));
      }
      if (syncRequests === 2) return retryResponse.response;
    }
    throw new Error("Unexpected request outside playlist resume");
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
  expect(screen.getByText("Paused", { exact: true })).toBeTruthy();
  const panel = within(
    screen.getByRole("region", { name: "Resume playlist updates" }),
  );
  expect(
    panel.getByText("The Spotify connection now also offers song search."),
  ).toBeTruthy();
  expect(panel.getByText("Abilities")).toBeTruthy();
  expect(panel.getByText("playlists", { exact: true })).toBeTruthy();
  expect(panel.getByText("playlists, search", { exact: true })).toBeTruthy();
  expect(panel.queryByText("Service address")).toBeNull();
  expect(
    panel.getByRole("link", { name: "Review connection" }).getAttribute("href"),
  ).toBe("/settings?section=plugins");
  expect(fetch.mock.calls.some(([path]) => path === "/api/v1/plugins")).toBe(
    false,
  );
  const table = screen.getByRole("table", { name: "Playlist entries" });
  const entries = () =>
    within(table)
      .getAllByRole("row")
      .slice(1)
      .map((row) => row.textContent);
  const originalEntries = entries();
  expect(syncRequests).toBe(0);
  const resume = panel.getByRole("button", { name: "Resume updates" });
  expect((resume as HTMLButtonElement).disabled).toBe(false);
  await act(async () => {
    fireEvent.click(resume);
  });
  expect(syncRequests).toBe(1);
  const posts = () =>
    fetch.mock.calls.filter(([path]) => path === "/api/v1/sync");
  const originalBody = String(posts()[0][1]?.body);
  const batch: Schema<"SyncBatch"> = JSON.parse(originalBody);
  expect(batch).toEqual({
    device_id: expect.any(String),
    operations: [
      {
        id: expect.stringMatching(
          /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
        ),
        action: {
          kind: "playlist_follow_resume",
          playlist_id: paused.playlist_id,
          expected_generation: paused.generation,
          installation_revision: 8,
          service_revision: 5,
        } satisfies Schema<"Action">,
      },
    ],
  });
  expect(screen.queryByText(rawError)).toBeNull();
  expect(entries()).toEqual(originalEntries);
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
  });
  expect(statusReads).toBe(2);
  const retry = screen.getByRole("button", { name: "Retry resume request" });
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
  expect((retry as HTMLButtonElement).disabled).toBe(true);
  await act(async () => {
    retryResponse.resolve(
      Response.json({
        acknowledged: [batch.operations[0].id],
      } satisfies Schema<"SyncReceipt">),
    );
  });
  expect(statusReads).toBe(3);
  expect(screen.getByText("Ready", { exact: true })).toBeTruthy();
  expect(
    screen.queryByRole("region", { name: "Resume playlist updates" }),
  ).toBeNull();
  expect(entries()).toEqual(originalEntries);
  expect(posts()).toHaveLength(2);
  expect(router.reload).not.toHaveBeenCalled();
  expect(router.visit).not.toHaveBeenCalled();
  expect(play).not.toHaveBeenCalled();
  const captured = JSON.stringify(logs.flatMap((sink) => sink.mock.calls));
  for (const privateValue of [
    rawError,
    paused.snapshot,
    props.detail.playlist.name,
    ...props.detail.entries.map((entry) => entry.title),
  ])
    expect(captured).not.toContain(privateValue);
  const beforeUnmount = fetch.mock.calls.length;
  view.unmount();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(fetch).toHaveBeenCalledTimes(beforeUnmount);
});

test("each kind of change is described in one plain sentence and a removed ability offers no resume", async () => {
  vi.useFakeTimers();
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const props = followedPage();
  const cases: [Partial<Schema<"PlaylistFollowReview">>, string, string[]][] = [
    [
      {
        changes: [
          { kind: "credential", before: ["No token"], after: ["Saved token"] },
        ],
      },
      "The saved access token for Spotify changed.",
      ["Access token", "No token", "Saved token"],
    ],
    [
      {
        resumable: false,
        changes: [
          {
            kind: "capabilities",
            before: ["playlists", "search"],
            after: ["search"],
          },
        ],
      },
      "The Spotify connection no longer offers playlists.",
      ["Abilities", "playlists, search", "search"],
    ],
    [
      {
        resumable: false,
        changes: [
          {
            kind: "removed",
            before: ["Connected"],
            after: ["Removed or replaced"],
          },
        ],
      },
      "The Spotify connection was removed or replaced.",
      ["Connection", "Connected", "Removed or replaced"],
    ],
  ];
  for (const [review, sentence, texts] of cases) {
    const paused = pausedFollow(props.detail.playlist.id, 3, review);
    vi.stubGlobal(
      "fetch",
      vi.fn((path: string) => {
        if (path.endsWith("/follow"))
          return Promise.resolve(Response.json(paused));
        throw new Error("Unexpected request outside review wording");
      }),
    );
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
    const panel = within(
      screen.getByRole("region", { name: "Resume playlist updates" }),
    );
    expect(panel.getByText(sentence)).toBeTruthy();
    for (const text of texts) expect(panel.getByText(text)).toBeTruthy();
    expect(
      panel.queryByRole("button", { name: "Resume updates" }) === null,
    ).toBe(review.resumable === false);
    expect(panel.getByRole("link", { name: "Review connection" })).toBeTruthy();
    view.unmount();
  }
});
