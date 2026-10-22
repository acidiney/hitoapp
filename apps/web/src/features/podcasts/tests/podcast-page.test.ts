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
import PodcastPage from "../pages/PodcastPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import type { components } from "../../../generated";

const playQueue = vi.fn(async () => {});
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => ({
    playQueue,
    subscribe: () => () => {},
    snapshot: () => ({ current: null }),
  }),
}));
vi.mock("@inertiajs/react", () => ({
  router: { visit: vi.fn(), reload: vi.fn() },
  Link: ({ children, href, ...attributes }: { children: ReactNode; href: string }) =>
    createElement("a", { href, ...attributes }, children),
}));

afterEach(() => {
  cleanup();
  playQueue.mockClear();
});

function podcastPageFixture() {
  const subscriptionId = "3668fe94-ed9b-4508-a3dc-bb051e889950";
  const first = {
    episode_id: "cd64956e-a534-4cfa-93c1-1cd424b1ae53",
    recording: {
      id: "e3af308a-236b-4812-9166-b29910949025",
      library_id: "e74b46d6-0b40-48ad-a7ae-4ec64cf9fcab",
      title: "Owned first episode",
      inherited_title: "Owned first episode",
      artist: "Owned podcast host",
      inherited_artist: "Owned podcast host",
      duration_ms: 120_000,
      liked: false,
      available: true,
    },
  } satisfies components["schemas"]["PodcastEpisode"];
  const second = {
    ...first,
    episode_id: "e6d71585-70cf-40bc-9885-86238b3e9b08",
    recording: {
      ...first.recording,
      id: "d74f9fb1-0f32-4724-99b7-4543dec2eb22",
      title: "Owned second episode",
      inherited_title: "Owned second episode",
    },
  } satisfies components["schemas"]["PodcastEpisode"];
  const props = {
    user: { id: "alice", username: "Alice", admin: false },
    errors: {},
    native_playback: false,
    libraries: [],
    playlists: [{ id: "owned-playlist", name: "My episodes", kind: "local" }],
    subscription: {
      subscription_id: subscriptionId,
      title: "Owned podcast",
      refresh_pending: false,
      refresh_failed: false,
      last_refreshed_at: 1_790_000_000,
      content_revision: 4,
    },
    episodes: {
      items: [first],
      total: 2,
      limit: 1,
      next_cursor: "owned-cursor/first?revision=4",
      content_revision: 4,
    },
    progress: [{ recording_id: first.recording.id, position_ms: 12_500, speed: 1.5 }],
  } satisfies components["schemas"]["PodcastPageProps"];
  return { subscriptionId, first, second, props };
}

test("Podcast episodes resume saved positions, start over explicitly and replace the shown page after progress loads", async () => {
  const { subscriptionId, first, second, props } = podcastPageFixture();
  let resolveProgress!: (response: Response) => void;
  const progressResponse = new Promise<Response>((resolve) => {
    resolveProgress = resolve;
  });
  const fetch = vi.fn<typeof globalThis.fetch>(async (input) => {
    const url = new URL(String(input), "http://hito.test");
    if (url.pathname === `/api/v1/podcasts/${subscriptionId}/episodes`) {
      expect(url.searchParams.get("limit")).toBe("1");
      expect(url.searchParams.get("after")).toBe(props.episodes.next_cursor);
      return Response.json({
        items: [second],
        total: 2,
        limit: 1,
        next_cursor: null,
        content_revision: 4,
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
  expect(playQueue).not.toHaveBeenCalled();
  const firstRow = within(screen.getByRole("table", { name: "Episodes" }))
    .getAllByRole("row")[1];
  expect(within(firstRow).getByRole("button", { name: "Like Owned first episode" })).toBeTruthy();
  expect(within(firstRow).getByRole("combobox", { name: "Add Owned first episode to playlist" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Resume Owned first episode" }));
  expect(playQueue).toHaveBeenLastCalledWith(
    [expect.objectContaining({ id: first.recording.id })],
    0,
    { kind: "saved", position: 12.5 },
  );
  fireEvent.click(screen.getByRole("button", { name: "Start over Owned first episode" }));
  expect(playQueue).toHaveBeenLastCalledWith(
    [expect.objectContaining({ id: first.recording.id })],
    0,
    { kind: "position", position: 0 },
  );
  playQueue.mockClear();
  fireEvent.click(screen.getByRole("button", { name: "Next episodes" }));
  await waitFor(() => expect(fetch).toHaveBeenCalledTimes(2));
  expect(fetch.mock.calls[0][1]).toMatchObject({
    method: "GET",
    headers: { "x-hito-account": "alice", "x-hito-request": "1" },
    signal: expect.any(AbortSignal),
  });
  expect(fetch.mock.calls[1][1]).toMatchObject({
    method: "POST",
    headers: { "x-hito-account": "alice", "x-hito-request": "1" },
    signal: expect.any(AbortSignal),
  });
  expect(JSON.parse(String(fetch.mock.calls[1][1]?.body))).toEqual({
    recording_ids: [second.recording.id],
  });
  expect(screen.getByRole("button", { name: "Resume Owned first episode" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Play Owned second episode" })).toBeNull();
  expect(playQueue).not.toHaveBeenCalled();

  await act(async () => resolveProgress(Response.json([])));
  await screen.findByRole("button", { name: "Play Owned second episode" });
  expect(screen.queryByRole("button", { name: "Resume Owned first episode" })).toBeNull();
  expect(within(screen.getByRole("table", { name: "Episodes" })).getAllByRole("row")).toHaveLength(2);
  fireEvent.click(screen.getByRole("button", { name: "Play Owned second episode" }));
  expect(playQueue).toHaveBeenLastCalledWith(
    [expect.objectContaining({ id: second.recording.id })],
    0,
    { kind: "saved", position: 0 },
  );
  expect(fetch).toHaveBeenCalledTimes(2);
  expect(screen.queryByRole("button", { name: /Download/ })).toBeNull();
});

test("Podcast episodes recover a stale cursor by loading a complete latest first page", async () => {
  const { subscriptionId, second, props } = podcastPageFixture();
  const latest = {
    ...second,
    recording: {
      ...second.recording,
      title: "Owned latest episode",
      inherited_title: "Owned latest episode",
    },
  } satisfies components["schemas"]["PodcastEpisode"];
  let resolveProgress!: (response: Response) => void;
  const progressResponse = new Promise<Response>((resolve) => {
    resolveProgress = resolve;
  });
  let episodeReads = 0;
  const fetch = vi.fn<typeof globalThis.fetch>(async (input) => {
    const url = new URL(String(input), "http://hito.test");
    if (url.pathname === `/api/v1/podcasts/${subscriptionId}/episodes`) {
      episodeReads += 1;
      expect(url.searchParams.get("limit")).toBe("1");
      if (episodeReads === 1) {
        expect(url.searchParams.get("after")).toBe(props.episodes.next_cursor);
        return Response.json({ error: "The feed changed." }, { status: 409 });
      }
      expect(url.searchParams.has("after")).toBe(false);
      return Response.json({
        items: [latest],
        total: 3,
        limit: 1,
        next_cursor: "owned-cursor/latest?revision=5",
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

  fireEvent.click(screen.getByRole("button", { name: "Next episodes" }));
  const loadLatest = await screen.findByRole("button", {
    name: "Load latest episodes",
  });
  expect(screen.getByRole("alert")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Resume Owned first episode" })).toBeTruthy();
  expect(fetch).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("button", { name: "Retry episodes" })).toBeNull();
  fireEvent.click(loadLatest);
  await waitFor(() => expect(fetch).toHaveBeenCalledTimes(3));
  expect(JSON.parse(String(fetch.mock.calls[2][1]?.body))).toEqual({
    recording_ids: [latest.recording.id],
  });
  expect(screen.getByRole("button", { name: "Resume Owned first episode" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Resume Owned latest episode" })).toBeNull();
  expect(playQueue).not.toHaveBeenCalled();

  await act(async () => {
    resolveProgress(Response.json([
      { recording_id: latest.recording.id, position_ms: 48_000, speed: 1.25 },
    ] satisfies components["schemas"]["RecordingProgress"][]));
  });
  await screen.findByRole("button", { name: "Resume Owned latest episode" });
  expect(screen.queryByRole("button", { name: "Resume Owned first episode" })).toBeNull();
  expect(within(screen.getByRole("table", { name: "Episodes" })).getAllByRole("row")).toHaveLength(2);
  expect(
    (screen.getByRole("button", { name: "First episodes" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  expect(screen.queryByRole("alert")).toBeNull();
  expect(fetch).toHaveBeenCalledTimes(3);
  expect(playQueue).not.toHaveBeenCalled();
});

test("Podcast episode reads abort on account change and unmount and ignore late private progress", async () => {
  const { subscriptionId, first, second, props } = podcastPageFixture();
  const bobSubscription = "1ef512a4-6279-41f7-aed5-280dfb3207c3";
  const bobEpisode = {
    ...first,
    episode_id: "48fc6289-6a7f-481f-86e2-e6e588ce3fe7",
    recording: {
      ...first.recording,
      id: "ff3bf4e0-619c-4f7f-bc59-ced43875655d",
      title: "Bob private episode",
      inherited_title: "Bob private episode",
    },
  } satisfies components["schemas"]["PodcastEpisode"];
  const bobLater = {
    ...bobEpisode,
    episode_id: "bb57c6e9-29fa-41ec-aae8-5805011ee33a",
    recording: {
      ...bobEpisode.recording,
      id: "9d681c39-9480-4575-80bf-c66b6ff5e97b",
      title: "Bob later episode",
      inherited_title: "Bob later episode",
    },
  } satisfies components["schemas"]["PodcastEpisode"];
  const bobProps = {
    ...props,
    user: { id: "bob", username: "Bob", admin: false },
    subscription: {
      ...props.subscription,
      subscription_id: bobSubscription,
      title: "Bob private podcast",
      content_revision: 7,
    },
    episodes: {
      ...props.episodes,
      items: [bobEpisode],
      next_cursor: "bob-private-cursor",
      content_revision: 7,
    },
    progress: [
      { recording_id: bobEpisode.recording.id, position_ms: 2_500, speed: 1 },
    ],
  } satisfies components["schemas"]["PodcastPageProps"];
  let resolveAliceProgress!: (response: Response) => void;
  const aliceProgress = new Promise<Response>((resolve) => {
    resolveAliceProgress = resolve;
  });
  let resolveBobProgress!: (response: Response) => void;
  const bobProgress = new Promise<Response>((resolve) => {
    resolveBobProgress = resolve;
  });
  const fetch = vi.fn<typeof globalThis.fetch>(async (input, options) => {
    const url = new URL(String(input), "http://hito.test");
    const account = new Headers(options?.headers).get("x-hito-account");
    if (url.pathname === `/api/v1/podcasts/${subscriptionId}/episodes`) {
      expect(account).toBe("alice");
      return Response.json({
        ...props.episodes,
        items: [second],
        next_cursor: null,
      });
    }
    if (url.pathname === `/api/v1/podcasts/${bobSubscription}/episodes`) {
      expect(account).toBe("bob");
      return Response.json({
        ...bobProps.episodes,
        items: [bobLater],
        next_cursor: null,
      });
    }
    if (url.pathname === "/api/v1/recordings/progress") {
      expect(JSON.parse(String(options?.body))).toEqual({
        recording_ids: [account === "alice" ? second.recording.id : bobLater.recording.id],
      });
      return account === "alice" ? aliceProgress : bobProgress;
    }
    throw new Error("Unexpected protocol request");
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(createElement(PodcastPage, props), {
    wrapper: ({ children }) => createElement(NoticeProvider, null, children),
  });

  fireEvent.click(screen.getByRole("button", { name: "Next episodes" }));
  await waitFor(() => expect(fetch).toHaveBeenCalledTimes(2));
  expect(fetch.mock.calls[0][1]?.signal?.aborted).toBe(false);
  expect(fetch.mock.calls[1][1]?.signal?.aborted).toBe(false);
  view.rerender(createElement(PodcastPage, bobProps));
  expect(fetch.mock.calls[0][1]?.signal?.aborted).toBe(true);
  expect(fetch.mock.calls[1][1]?.signal?.aborted).toBe(true);
  expect(screen.getByRole("heading", { name: "Bob private podcast" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Resume Bob private episode" })).toBeTruthy();
  expect(screen.queryByText(first.recording.title)).toBeNull();
  expect(fetch).toHaveBeenCalledTimes(2);

  await act(async () => {
    resolveAliceProgress(Response.json([
      { recording_id: second.recording.id, position_ms: 36_000, speed: 1.25 },
    ] satisfies components["schemas"]["RecordingProgress"][]));
  });
  expect(screen.getByRole("button", { name: "Resume Bob private episode" })).toBeTruthy();
  expect(screen.queryByText(first.recording.title)).toBeNull();
  expect(screen.queryByText(second.recording.title)).toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();
  expect(playQueue).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Next episodes" }));
  await waitFor(() => expect(fetch).toHaveBeenCalledTimes(4));
  expect(fetch.mock.calls[2][1]?.signal?.aborted).toBe(false);
  expect(fetch.mock.calls[3][1]?.signal?.aborted).toBe(false);
  view.unmount();
  expect(fetch.mock.calls[2][1]?.signal?.aborted).toBe(true);
  expect(fetch.mock.calls[3][1]?.signal?.aborted).toBe(true);
  await act(async () => resolveBobProgress(Response.json([])));
  expect(fetch).toHaveBeenCalledTimes(4);
  expect(playQueue).not.toHaveBeenCalled();
});
