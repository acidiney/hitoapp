// @vitest-environment jsdom
import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import PodcastPage from "./pages/PodcastPage";
import { NoticeBanner, NoticeProvider } from "./components/Notices";
import type { components } from "./generated";

const playQueue = vi.fn(async () => {});
const clear = vi.fn();
vi.mock("./player/PlaybackProvider", () => ({
  usePlayer: () => ({
    playQueue,
    clear,
    subscribe: () => () => {},
    snapshot: () => ({ current: null }),
  }),
}));
vi.mock("@inertiajs/react", () => ({
  router: { visit: vi.fn(), reload: vi.fn(), cancelAll: vi.fn() },
  Link: ({
    children,
    href,
    ...attributes
  }: {
    children: ReactNode;
    href: string;
  }) => createElement("a", { href, ...attributes }, children),
}));

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
  localStorage.clear();
});

test("Podcast unsubscribe cancels its already-issued Like reload before sending the immutable request", async () => {
  const episode = {
    episode_id: "d08f4483-f619-44ee-957a-90dc25e92482",
    recording: {
      id: "a1e898a1-3322-426f-a1ef-c7ea109de1a7",
      library_id: "ef1567ed-0f60-4d04-b0f6-465696603f94",
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
      subscription_id: "fb9727dd-319a-409c-8c1c-961162756193",
      title: "Owned podcast",
      refresh_pending: false,
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
  const events: string[] = [];
  let cancelled = false;
  const cancelReload = vi.fn(() => {
    cancelled = true;
    events.push("reload.cancelled");
  });
  vi.mocked(router.reload).mockImplementation((options) => {
    events.push("reload.started");
    options?.onCancelToken?.({ cancel: cancelReload });
  });
  const unsubscribePath = `/api/v1/podcasts/${props.subscription.subscription_id}/unsubscribe`;
  let originalBody: string | null = null;
  let resolveReceipt!: (response: Response) => void;
  const receiptResponse = new Promise<Response>((resolve) => {
    resolveReceipt = resolve;
  });
  let liked = false;
  const fetch = vi.fn<typeof globalThis.fetch>(async (input, options) => {
    const path = String(input);
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
    if (path === unsubscribePath) {
      events.push("unsubscribe.sent");
      const body = String(options?.body);
      if (originalBody === null) {
        originalBody = body;
        throw new TypeError("The accepted unsubscribe response was lost.");
      }
      expect(body).toBe(originalBody);
      return receiptResponse;
    }
    throw new Error("Unexpected protocol request");
  });
  vi.stubGlobal("fetch", fetch);
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
    fireEvent.click(
      screen.getByRole("button", { name: "Like Owned retained episode" }),
    );
  });
  expect(liked).toBe(true);
  expect(router.reload).toHaveBeenCalledTimes(1);
  expect(cancelled).toBe(false);
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Unsubscribe" }));
  });
  expect(events).toEqual([
    "reload.started",
    "reload.cancelled",
    "unsubscribe.sent",
  ]);
  expect(cancelReload).toHaveBeenCalledTimes(1);
  expect(router.cancelAll).not.toHaveBeenCalled();
  expect(router.visit).not.toHaveBeenCalled();
  expect(liked).toBe(true);
  expect(screen.queryByRole("button", { name: "Dismiss message" })).toBeNull();
  const request = JSON.parse(originalBody!);
  await act(async () => {
    fireEvent.click(
      screen.getByRole("button", { name: "Retry unsubscribe request" }),
    );
  });
  expect(fetch).toHaveBeenCalledTimes(3);
  expect(String(fetch.mock.calls[2][1]?.body)).toBe(originalBody);
  await act(async () => {
    resolveReceipt(
      Response.json({
        request_id: request.request_id,
        subscription_id: props.subscription.subscription_id,
      } satisfies components["schemas"]["PodcastUnsubscribeReceipt"]),
    );
  });
  expect(router.visit).toHaveBeenCalledExactlyOnceWith("/podcasts");
  expect(router.reload).toHaveBeenCalledTimes(1);
  expect(cancelReload).toHaveBeenCalledTimes(1);
  expect(router.cancelAll).not.toHaveBeenCalled();
  expect(liked).toBe(true);
  expect(clear).not.toHaveBeenCalled();
  expect(playQueue).not.toHaveBeenCalled();
});
