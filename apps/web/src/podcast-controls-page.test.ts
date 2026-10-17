// @vitest-environment jsdom
import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import PodcastPage from "./pages/PodcastPage";
import { NoticeProvider } from "./components/Notices";
import type { components } from "./generated";

const playQueue = vi.fn(async () => {});
vi.mock("./player/PlaybackProvider", () => ({
  usePlayer: () => ({
    playQueue,
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

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  playQueue.mockClear();
  vi.mocked(router.reload).mockClear();
});

function podcastControlsFixture() {
  const episode = {
    episode_id: "4756cdd8-7e9a-49f4-8db4-2f64a38dd30c",
    recording: {
      id: "33ef7752-1783-4e31-8da9-edc201e043fd",
      library_id: "511b1691-6a9f-4ec8-9766-04b5d3c63747",
      title: "Owned prior episode",
      inherited_title: "Owned prior episode",
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
      subscription_id: "7e1b22c7-7aa1-48b4-8b4b-418475a03a8d",
      title: "Owned podcast",
      refresh_pending: false,
      refresh_failed: false,
      last_refreshed_at: 1_790_000_000,
      content_revision: 4,
    },
    episodes: {
      items: [episode],
      total: 2,
      limit: 1,
      next_cursor: "owned-old-cursor",
      content_revision: 4,
    },
    progress: [
      { recording_id: episode.recording.id, position_ms: 12_500, speed: 1.5 },
    ],
  } satisfies components["schemas"]["PodcastPageProps"];
  return { props, episode };
}

test("Podcast refresh retries a lost acknowledgement exactly and replaces episodes only after ready status and progress", async () => {
  const { props, episode } = podcastControlsFixture();
  const latest = {
    ...episode,
    episode_id: "995cfebf-07ce-434c-b4a5-2a9cd3d97e54",
    recording: {
      ...episode.recording,
      id: "7e2b6441-2f01-4646-9699-2ddc1dbd769d",
      title: "Owned refreshed episode",
      inherited_title: "Owned refreshed episode",
    },
  } satisfies components["schemas"]["PodcastEpisode"];
  const subscription = props.subscription.subscription_id;
  let originalBody: string | null = null;
  let statusReads = 0;
  let resolveProgress!: (response: Response) => void;
  const progressResponse = new Promise<Response>((resolve) => {
    resolveProgress = resolve;
  });
  const fetch = vi.fn<typeof globalThis.fetch>(async (input, options) => {
    const url = new URL(String(input), "http://hito.test");
    if (url.pathname === `/api/v1/podcasts/${subscription}/refresh`) {
      const body = String(options?.body);
      if (originalBody === null) {
        originalBody = body;
        throw new TypeError("The accepted response was lost.");
      }
      expect(body).toBe(originalBody);
      const request = JSON.parse(body);
      return Response.json(
        {
          request_id: request.request_id,
          subscription_id: subscription,
          job_id: "2c76c7af-f621-461d-a684-5b6c337727e8",
        } satisfies components["schemas"]["PodcastRefreshReceipt"],
        { status: 202 },
      );
    }
    if (url.pathname === "/api/v1/podcasts") {
      statusReads += 1;
      return Response.json([
        {
          ...props.subscription,
          refresh_pending: statusReads === 1,
          content_revision: statusReads === 1 ? 4 : 5,
          last_refreshed_at: statusReads === 1 ? 1_790_000_000 : 1_790_000_100,
        } satisfies components["schemas"]["PodcastSubscription"],
      ]);
    }
    if (url.pathname === `/api/v1/podcasts/${subscription}/episodes`) {
      expect(url.searchParams.has("after")).toBe(false);
      expect(url.searchParams.get("limit")).toBe("1");
      return Response.json({
        items: [latest],
        total: 3,
        limit: 1,
        next_cursor: "owned-current-cursor",
        content_revision: 5,
      } satisfies components["schemas"]["PodcastEpisodePage"]);
    }
    if (url.pathname === "/api/v1/recordings/progress") return progressResponse;
    throw new Error("Unexpected protocol request");
  });
  vi.stubGlobal("fetch", fetch);
  render(createElement(PodcastPage, props), {
    wrapper: ({ children }) => createElement(NoticeProvider, null, children),
  });

  expect(fetch).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Refresh feed" }));
  await screen.findByRole("alert");
  expect(fetch).toHaveBeenCalledTimes(1);
  expect(fetch.mock.calls[0][1]).toMatchObject({
    method: "POST",
    credentials: "same-origin",
    headers: { "x-hito-account": "alice", "x-hito-request": "1" },
    signal: expect.any(AbortSignal),
  });
  expect(JSON.parse(originalBody!)).toEqual({
    request_id: expect.stringMatching(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    ),
  });
  expect(
    screen.getByRole("button", { name: "Resume Owned prior episode" }),
  ).toBeTruthy();
  vi.useFakeTimers();
  await act(async () => {
    fireEvent.click(
      screen.getByRole("button", { name: "Retry refresh request" }),
    );
  });
  expect(fetch).toHaveBeenCalledTimes(3);
  expect(String(fetch.mock.calls[1][1]?.body)).toBe(originalBody);
  expect(fetch.mock.calls[1][1]?.signal).not.toBe(
    fetch.mock.calls[0][1]?.signal,
  );
  expect(screen.getByText("Checking feed…")).toBeTruthy();
  expect(statusReads).toBe(1);
  expect(playQueue).not.toHaveBeenCalled();

  await act(async () => {
    await vi.advanceTimersByTimeAsync(5_000);
  });
  expect(statusReads).toBe(2);
  expect(fetch).toHaveBeenCalledTimes(6);
  expect(JSON.parse(String(fetch.mock.calls[5][1]?.body))).toEqual({
    recording_ids: [latest.recording.id],
  });
  expect(
    screen.getByRole("button", { name: "Resume Owned prior episode" }),
  ).toBeTruthy();
  expect(
    screen.queryByRole("button", { name: "Resume Owned refreshed episode" }),
  ).toBeNull();
  expect(playQueue).not.toHaveBeenCalled();
  await act(async () => {
    resolveProgress(
      Response.json([
        { recording_id: latest.recording.id, position_ms: 48_000, speed: 1.25 },
      ] satisfies components["schemas"]["RecordingProgress"][]),
    );
  });
  expect(
    screen.getByRole("button", { name: "Resume Owned refreshed episode" }),
  ).toBeTruthy();
  expect(
    screen.queryByRole("button", { name: "Resume Owned prior episode" }),
  ).toBeNull();
  expect(
    within(screen.getByRole("table", { name: "Episodes" })).getAllByRole("row"),
  ).toHaveLength(2);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(statusReads).toBe(2);
  expect(fetch).toHaveBeenCalledTimes(6);
  expect(playQueue).not.toHaveBeenCalled();
  expect(screen.queryByRole("button", { name: /Download/ })).toBeNull();
});

test("Podcast refresh keeps its current revision when an earlier Like reload delivers stale server props", async () => {
  const { props, episode } = podcastControlsFixture();
  const subscription = props.subscription.subscription_id;
  const latest = {
    ...episode,
    recording: {
      ...episode.recording,
      title: "Owned current episode",
      inherited_title: "Owned current episode",
      liked: true,
    },
  } satisfies components["schemas"]["PodcastEpisode"];
  const capturedLikeProps = {
    ...props,
    subscription: { ...props.subscription },
    episodes: {
      ...props.episodes,
      items: [{ ...episode, recording: { ...episode.recording, liked: true } }],
    },
    progress: props.progress.map((item) => ({ ...item })),
  } satisfies components["schemas"]["PodcastPageProps"];
  let resolveProgress!: (response: Response) => void;
  const progressResponse = new Promise<Response>((resolve) => {
    resolveProgress = resolve;
  });
  let progressReads = 0;
  const fetch = vi.fn<typeof globalThis.fetch>(async (input, options) => {
    const url = new URL(String(input), "http://hito.test");
    if (url.pathname === "/api/v1/sync") {
      const body: components["schemas"]["SyncBatch"] = JSON.parse(
        String(options?.body),
      );
      expect(body.operations[0].action).toEqual({
        kind: "like_set",
        recording_id: episode.recording.id,
        liked: true,
      });
      return Response.json({
        acknowledged: body.operations.map((operation) => operation.id),
      });
    }
    if (url.pathname === `/api/v1/podcasts/${subscription}/refresh`) {
      const body = JSON.parse(String(options?.body));
      return Response.json(
        {
          request_id: body.request_id,
          subscription_id: subscription,
          job_id: "5c7d9084-d64d-4782-8cdf-b318c0e07e4e",
        } satisfies components["schemas"]["PodcastRefreshReceipt"],
        { status: 202 },
      );
    }
    if (url.pathname === "/api/v1/podcasts")
      return Response.json([
        {
          ...props.subscription,
          content_revision: 5,
          last_refreshed_at: 1_790_000_100,
        } satisfies components["schemas"]["PodcastSubscription"],
      ]);
    if (url.pathname === `/api/v1/podcasts/${subscription}/episodes`) {
      expect(url.searchParams.has("after")).toBe(false);
      return Response.json({
        ...props.episodes,
        items: [latest],
        total: 3,
        next_cursor: "owned-current-cursor",
        content_revision: 5,
      } satisfies components["schemas"]["PodcastEpisodePage"]);
    }
    if (url.pathname === "/api/v1/recordings/progress") {
      progressReads += 1;
      return progressResponse;
    }
    throw new Error("Unexpected protocol request");
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(createElement(PodcastPage, props), {
    wrapper: ({ children }) => createElement(NoticeProvider, null, children),
  });

  fireEvent.click(
    screen.getByRole("button", { name: "Like Owned prior episode" }),
  );
  await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(1));
  fireEvent.click(screen.getByRole("button", { name: "Refresh feed" }));
  await waitFor(() => expect(progressReads).toBe(1));
  expect(
    screen.getByRole("button", { name: "Resume Owned prior episode" }),
  ).toBeTruthy();
  expect(
    screen.queryByRole("button", { name: "Resume Owned current episode" }),
  ).toBeNull();
  view.rerender(createElement(PodcastPage, capturedLikeProps));
  await act(async () => {
    resolveProgress(
      Response.json([
        { recording_id: latest.recording.id, position_ms: 48_000, speed: 1.25 },
      ] satisfies components["schemas"]["RecordingProgress"][]),
    );
  });
  await screen.findByRole("button", { name: "Resume Owned current episode" });
  expect(
    screen.getByRole("button", { name: "Unlike Owned current episode" }),
  ).toBeTruthy();
  expect(
    screen.queryByRole("button", { name: "Resume Owned prior episode" }),
  ).toBeNull();
  expect(screen.getByText("1 to 1 of 3")).toBeTruthy();
  expect(screen.queryByRole("alert")).toBeNull();
  expect(playQueue).not.toHaveBeenCalled();
});

test("Podcast ready server props after an unchanged feed check release the retained archive cursor", async () => {
  const { props, episode } = podcastControlsFixture();
  const pendingProps = {
    ...props,
    subscription: { ...props.subscription, refresh_pending: true },
  } satisfies components["schemas"]["PodcastPageProps"];
  const readyProps = {
    ...props,
    subscription: {
      ...props.subscription,
      refresh_pending: false,
      last_refreshed_at: 1_790_000_100,
    },
    episodes: {
      ...props.episodes,
      items: [{ ...episode, recording: { ...episode.recording, liked: true } }],
    },
    progress: props.progress.map((item) => ({ ...item })),
  } satisfies components["schemas"]["PodcastPageProps"];
  const next = {
    ...episode,
    episode_id: "bff954ca-1a03-479a-bd58-c797f9aad330",
    recording: {
      ...episode.recording,
      id: "132f1b89-dc94-4a6d-beb2-e11c97c32c90",
      title: "Owned next episode",
      inherited_title: "Owned next episode",
    },
  } satisfies components["schemas"]["PodcastEpisode"];
  const fetch = vi.fn<typeof globalThis.fetch>(async (input, options) => {
    const url = new URL(String(input), "http://hito.test");
    if (
      url.pathname ===
      `/api/v1/podcasts/${props.subscription.subscription_id}/episodes`
    ) {
      expect(url.searchParams.get("after")).toBe(props.episodes.next_cursor);
      return Response.json({
        ...props.episodes,
        items: [next],
        next_cursor: null,
      } satisfies components["schemas"]["PodcastEpisodePage"]);
    }
    if (url.pathname === "/api/v1/recordings/progress") {
      expect(JSON.parse(String(options?.body))).toEqual({
        recording_ids: [next.recording.id],
      });
      return Response.json([]);
    }
    throw new Error("Unexpected protocol request");
  });
  vi.stubGlobal("fetch", fetch);
  vi.useFakeTimers();
  const view = render(createElement(PodcastPage, pendingProps), {
    wrapper: ({ children }) => createElement(NoticeProvider, null, children),
  });
  expect(
    (screen.getByRole("button", { name: "Next episodes" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  expect(fetch).not.toHaveBeenCalled();
  view.rerender(createElement(PodcastPage, readyProps));
  expect(
    screen.getByRole("button", { name: "Unlike Owned prior episode" }),
  ).toBeTruthy();
  expect(screen.queryByText("Checking feed…")).toBeNull();
  expect(fetch).not.toHaveBeenCalled();
  expect(
    (screen.getByRole("button", { name: "Next episodes" }) as HTMLButtonElement)
      .disabled,
  ).toBe(false);
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Next episodes" }));
  });
  expect(
    screen.getByRole("button", { name: "Play Owned next episode" }),
  ).toBeTruthy();
  expect(fetch).toHaveBeenCalledTimes(2);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(fetch).toHaveBeenCalledTimes(2);
  expect(playQueue).not.toHaveBeenCalled();
});
