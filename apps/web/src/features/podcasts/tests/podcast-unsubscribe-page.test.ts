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
import PodcastPage from "../pages/PodcastPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import type { PlaybackView } from "../../playback/model";
import type { components } from "../../../generated";

const playback: PlaybackView = {
  current: null,
  playing: false,
  loading: false,
  error: "",
  repeat: "off",
  shuffle: false,
  speed: 1,
  volume: 1,
  connection: "ready",
};
const playQueue = vi.fn(async () => {});
const clear = vi.fn(() => {
  playback.current = null;
  playback.playing = false;
});
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => ({
    playQueue,
    clear,
    subscribe: () => () => {},
    snapshot: () => playback,
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

afterEach(() => {
  cleanup();
  playback.current = null;
  playback.playing = false;
});

test("Podcast unsubscribe recovers its exact receipt after status omits the subscription and preserves the shared player", async () => {
  const episode = {
    episode_id: "3fda5144-7fe4-4da7-aafa-d8809eb29aac",
    recording: {
      id: "16050c75-9c4b-49bd-807e-fce128103d3d",
      library_id: "e85ea237-0e7e-4ae5-983a-77297e9c5a50",
      title: "Owned retained episode",
      inherited_title: "Owned retained episode",
      artist: "Owned host",
      inherited_artist: "Owned host",
      duration_ms: 120_000,
      liked: true,
      available: true,
    },
  } satisfies components["schemas"]["PodcastEpisode"];
  const props = {
    user: { id: "alice", username: "Alice", admin: false },
    errors: {},
    native_playback: false,
    lock: { pin_enabled: false, idle_minutes: 0, locked: false },
    libraries: [],
    playlists: [
      { id: "owned-playlist", name: "Retained episodes", kind: "local" },
    ],
    subscription: {
      subscription_id: "2e92a5db-03bd-4a49-897c-e9f2799802bc",
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
  const path = `/api/v1/podcasts/${props.subscription.subscription_id}/unsubscribe`;
  let originalBody: string | null = null;
  let resolveReceipt!: (response: Response) => void;
  const receiptResponse = new Promise<Response>((resolve) => {
    resolveReceipt = resolve;
  });
  let statusReads = 0;
  const fetch = vi.fn<typeof globalThis.fetch>(async (input, options) => {
    if (String(input) === path) {
      const body = String(options?.body);
      if (originalBody === null) {
        originalBody = body;
        throw new TypeError("The accepted unsubscribe response was lost.");
      }
      expect(body).toBe(originalBody);
      return receiptResponse;
    }
    if (String(input) === "/api/v1/podcasts") {
      statusReads += 1;
      return Response.json([]);
    }
    throw new Error("Unexpected protocol request");
  });
  vi.stubGlobal("fetch", fetch);
  vi.useFakeTimers();
  playback.current = episode.recording;
  playback.playing = true;
  render(createElement(PodcastPage, props), {
    wrapper: ({ children }) => createElement(NoticeProvider, null, children),
  });

  expect(fetch).not.toHaveBeenCalled();
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Unsubscribe" }));
  });
  expect(screen.getByRole("alert")).toBeTruthy();
  expect(fetch).toHaveBeenCalledTimes(1);
  expect(fetch.mock.calls[0][1]).toMatchObject({
    method: "POST",
    credentials: "same-origin",
    headers: { "x-hito-account": "alice", "x-hito-request": "1" },
    signal: expect.any(AbortSignal),
  });
  const request = JSON.parse(originalBody!);
  expect(request).toEqual({
    request_id: expect.stringMatching(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    ),
  });
  expect(router.visit).not.toHaveBeenCalled();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(5_000);
  });
  expect(statusReads).toBe(1);
  expect(fetch).toHaveBeenCalledTimes(2);
  expect(
    screen.getByRole("button", { name: "Resume Owned retained episode" }),
  ).toBeTruthy();
  expect(
    screen.getByRole("button", { name: "Unlike Owned retained episode" }),
  ).toBeTruthy();
  expect(
    screen.getByRole("combobox", {
      name: "Add Owned retained episode to playlist",
    }),
  ).toBeTruthy();
  expect(playback.current).toBe(episode.recording);
  expect(playback.playing).toBe(true);
  expect(clear).not.toHaveBeenCalled();
  expect(router.visit).not.toHaveBeenCalled();
  await act(async () => {
    fireEvent.click(
      screen.getByRole("button", { name: "Retry unsubscribe request" }),
    );
  });
  expect(fetch).toHaveBeenCalledTimes(3);
  expect(fetch.mock.calls[2][0]).toBe(path);
  expect(String(fetch.mock.calls[2][1]?.body)).toBe(originalBody);
  expect(fetch.mock.calls[2][1]).toMatchObject({
    method: "POST",
    headers: { "x-hito-account": "alice", "x-hito-request": "1" },
  });
  expect(fetch.mock.calls[2][1]?.signal).not.toBe(
    fetch.mock.calls[0][1]?.signal,
  );
  expect(router.visit).not.toHaveBeenCalled();
  await act(async () => {
    resolveReceipt(
      Response.json({
        request_id: request.request_id,
        subscription_id: props.subscription.subscription_id,
      } satisfies components["schemas"]["PodcastUnsubscribeReceipt"]),
    );
  });
  expect(router.visit).toHaveBeenCalledExactlyOnceWith("/podcasts");
  expect(playback.current).toBe(episode.recording);
  expect(playback.playing).toBe(true);
  expect(clear).not.toHaveBeenCalled();
  expect(playQueue).not.toHaveBeenCalled();
  expect(fetch).toHaveBeenCalledTimes(3);
  expect(screen.queryByRole("button", { name: /Download/ })).toBeNull();
});
