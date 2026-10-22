import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import PodcastPage from "../pages/PodcastPage";
import { NoticeBanner, NoticeProvider } from "../../../shared/ui/Notices";
import type { components } from "../../../generated";

const playQueue = vi.fn(async () => {});
const clear = vi.fn();
vi.mock("../../../player/PlaybackProvider", () => ({
  usePlayer: () => ({
    playQueue,
    clear,
    subscribe: () => () => {},
    snapshot: () => ({ current: null }),
  }),
}));
vi.mock("@inertiajs/react", () => ({
  router: { visit: vi.fn(), reload: vi.fn() },
  Link: ({
    children,
    href,
    ...attributes
  }: {
    children: ReactNode;
    href: string;
  }) => createElement("a", { href, ...attributes }, children),
}));

test("A retained episode Like succeeds without reloading inactive podcast detail during unsubscribe recovery", async () => {
  const episode = {
    episode_id: "040ec411-6748-4cde-8967-f5bf94640991",
    recording: {
      id: "395f8dc6-03d3-4ebe-b354-d0111a35a494",
      library_id: "493057fa-21cd-4935-9a93-8c92c5f148ad",
      title: "Owned retained episode",
      inherited_title: "Owned retained episode",
      artist: "Owned host",
      inherited_artist: "Owned host",
      duration_ms: 120_000,
      liked: false,
      available: true,
    },
  } satisfies components["schemas"]["PodcastEpisode"];
  const props = {
    user: { id: "alice", username: "Alice", admin: false },
    errors: {},
    native_playback: false,
    libraries: [],
    playlists: [],
    subscription: {
      subscription_id: "067c8d6e-2193-4bb7-89ac-a684ce80e207",
      title: "Owned podcast",
      refresh_pending: true,
      refresh_failed: false,
      last_refreshed_at: 1_790_000_000,
      content_revision: 4,
    },
    episodes: {
      items: [episode],
      total: 1,
      limit: 1,
      next_cursor: null,
      content_revision: 4,
    },
    progress: [
      { recording_id: episode.recording.id, position_ms: 12_500, speed: 1.5 },
    ],
  } satisfies components["schemas"]["PodcastPageProps"];
  const unsubscribePath = `/api/v1/podcasts/${props.subscription.subscription_id}/unsubscribe`;
  let originalBody: string | null = null;
  let resolveReceipt!: (response: Response) => void;
  const receiptResponse = new Promise<Response>((resolve) => {
    resolveReceipt = resolve;
  });
  let statusReads = 0;
  let liked = false;
  const fetch = vi.fn<typeof globalThis.fetch>(async (input, options) => {
    const path = String(input);
    if (path === unsubscribePath) {
      const body = String(options?.body);
      if (originalBody === null) {
        originalBody = body;
        throw new TypeError("The accepted unsubscribe response was lost.");
      }
      expect(body).toBe(originalBody);
      return receiptResponse;
    }
    if (path === "/api/v1/podcasts") {
      statusReads += 1;
      return Response.json([]);
    }
    if (path === "/api/v1/sync") {
      const body: components["schemas"]["SyncBatch"] = JSON.parse(
        String(options?.body),
      );
      expect(body.operations[0].action).toEqual({
        kind: "like_set",
        recording_id: episode.recording.id,
        liked: true,
      });
      liked = true;
      return Response.json({
        acknowledged: body.operations.map((operation) => operation.id),
      });
    }
    throw new Error("Unexpected protocol request");
  });
  vi.stubGlobal("fetch", fetch);
  vi.useFakeTimers();
  render(createElement(PodcastPage, props), {
    wrapper: ({ children }) =>
      createElement(
        NoticeProvider,
        null,
        children,
        createElement(NoticeBanner),
      ),
  });

  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Unsubscribe" }));
  });
  expect(
    screen.getByRole("button", { name: "Retry unsubscribe request" }),
  ).toBeTruthy();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(5_000);
  });
  expect(statusReads).toBe(1);
  expect(router.visit).not.toHaveBeenCalled();
  await act(async () => {
    fireEvent.click(
      screen.getByRole("button", { name: "Like Owned retained episode" }),
    );
  });
  expect(liked).toBe(true);
  expect(fetch.mock.calls[2][0]).toBe("/api/v1/sync");
  expect(fetch.mock.calls[2][1]).toMatchObject({
    method: "POST",
    headers: { "x-hito-account": "alice", "x-hito-request": "1" },
  });
  expect(router.reload).not.toHaveBeenCalled();
  expect(router.visit).not.toHaveBeenCalled();
  expect(screen.queryByRole("button", { name: "Dismiss message" })).toBeNull();
  expect(
    screen.getByRole("button", { name: "Retry unsubscribe request" }),
  ).toBeTruthy();
  const request = JSON.parse(originalBody!);
  await act(async () => {
    fireEvent.click(
      screen.getByRole("button", { name: "Retry unsubscribe request" }),
    );
  });
  expect(fetch).toHaveBeenCalledTimes(4);
  expect(fetch.mock.calls[3][0]).toBe(unsubscribePath);
  expect(String(fetch.mock.calls[3][1]?.body)).toBe(originalBody);
  await act(async () => {
    resolveReceipt(
      Response.json({
        request_id: request.request_id,
        subscription_id: props.subscription.subscription_id,
      } satisfies components["schemas"]["PodcastUnsubscribeReceipt"]),
    );
  });
  expect(router.visit).toHaveBeenCalledExactlyOnceWith("/podcasts");
  expect(router.reload).not.toHaveBeenCalled();
  expect(liked).toBe(true);
  expect(clear).not.toHaveBeenCalled();
  expect(playQueue).not.toHaveBeenCalled();
});
