import type { Schema } from "../../../shared/schema";
import { createElement, useState, type AnchorHTMLAttributes } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ArtistPage from "../pages/ArtistPage";

vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => ({
    playQueue: vi.fn(),
    subscribe: () => () => {},
    snapshot: () => ({ current: null }),
    subscribePosition: vi.fn(),
  }),
}));
vi.mock("../../../shared/hooks/useMutation", () => ({
  useMutation: () => ({ mutate: vi.fn(), pending: false }),
}));
vi.mock("@inertiajs/react", () => ({
  router: { get: vi.fn() },
  useRemember: (initial: unknown) => useState(initial),
  usePage: () => ({ url: window.location.pathname }),
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function recording(id: string, title: string) {
  return {
    id,
    library_id: "personal",
    release_id: null,
    title,
    inherited_title: title,
    artist: "Ada feat. Cy",
    inherited_artist: "Ada feat. Cy",
    album: null,
    duration_ms: 1000,
    available: true,
    plays_from_services: false,
    liked: false,
  };
}

function page(
  main: ReturnType<typeof recording>[],
  featured: ReturnType<typeof recording>[],
  featuredTotal = featured.length,
  memberOnly = false,
): Schema<"ArtistPageProps"> {
  return {
    errors: {},
    native_playback: false,
    lock: { pin_enabled: false, idle_minutes: 0, locked: false },
    user: { id: "account", username: "Listener", admin: false },
    libraries: [],
    playlists: [],
    detail: {
      artist: {
        id: "cy",
        library_id: "personal",
        name: "Cy",
        release_count: 0,
        recording_count: main.length,
        featured_recording_count: featuredTotal,
        ...(memberOnly ? { member_only: true } : {}),
      },
      releases: { total: 0, offset: 0, limit: 60, items: [] },
      recordings: { total: main.length, offset: 0, limit: 60, items: main },
      featured_recordings: {
        total: featuredTotal,
        offset: 0,
        limit: 60,
        items: featured,
      },
    },
  };
}

test("a featured artist lists its guest appearances under their own heading", () => {
  render(createElement(ArtistPage, page([], [recording("r", "Guest verse")])));
  expect(screen.getByRole("heading", { name: "Appears on" })).toBeTruthy();
  expect(screen.getByText("Guest verse")).toBeTruthy();
  expect(screen.queryByRole("heading", { name: "Tracks" })).toBeNull();
});

test("main tracks and appearances stay in separate sections and an empty one is not shown", () => {
  const { unmount } = render(
    createElement(
      ArtistPage,
      page([recording("main", "Own song")], [recording("r", "Guest verse")]),
    ),
  );
  expect(screen.getByRole("heading", { name: "Tracks" })).toBeTruthy();
  expect(screen.getByRole("heading", { name: "Appears on" })).toBeTruthy();
  unmount();
  render(createElement(ArtistPage, page([recording("main", "Own song")], [])));
  expect(screen.queryByRole("heading", { name: "Appears on" })).toBeNull();
});

test("a longer list of appearances says how many are shown", () => {
  render(
    createElement(ArtistPage, page([], [recording("r", "Guest verse")], 75)),
  );
  expect(screen.getByText("Showing 1 of 75 appearances")).toBeTruthy();
});

test("an artist only named inside credits has no credit of its own to identify", () => {
  const { unmount } = render(
    createElement(
      ArtistPage,
      page([], [recording("r", "Guest verse")], 1, true),
    ),
  );
  expect(screen.queryByText("Artist identity")).toBeNull();
  unmount();
  render(createElement(ArtistPage, page([recording("main", "Own song")], [])));
  expect(screen.getByText("Artist identity")).toBeTruthy();
});

test("an artist only named inside credits says it appears in the library rather than that it has a credit", () => {
  const { unmount } = render(
    createElement(
      ArtistPage,
      page([], [recording("r", "Guest verse")], 1, true),
    ),
  );
  expect(screen.getByText("1 appearance")).toBeTruthy();
  unmount();
  render(createElement(ArtistPage, page([recording("main", "Own song")], [])));
  expect(screen.getByText("1 track")).toBeTruthy();
});

test("Load more appends the next appearances from the artist endpoint", async () => {
  const fetch = vi.fn(async (_path: string) =>
    Response.json({
      ...page([], [recording("second", "Second verse")], 2).detail,
      featured_recordings: {
        total: 2,
        offset: 1,
        limit: 60,
        items: [recording("second", "Second verse")],
      },
    }),
  );
  vi.stubGlobal("fetch", fetch);
  render(
    createElement(
      ArtistPage,
      page([], [recording("first", "First verse")], 2, true),
    ),
  );
  expect(screen.getByText("Showing 1 of 2 appearances")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Load more" }));
  await waitFor(() => expect(screen.getByText("Second verse")).toBeTruthy());
  expect(screen.getByText("First verse")).toBeTruthy();
  expect(fetch.mock.calls[0][0]).toBe(
    "/api/v1/artists/cy?featured_offset=1&limit=60",
  );
  expect(screen.queryByRole("button", { name: "Load more" })).toBeNull();
});

test("a failed load of more appearances can be retried", async () => {
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(Response.json({ message: "Down" }, { status: 503 }))
    .mockResolvedValueOnce(
      Response.json({
        ...page([], [recording("second", "Second verse")], 2).detail,
        featured_recordings: {
          total: 2,
          offset: 1,
          limit: 60,
          items: [recording("second", "Second verse")],
        },
      }),
    );
  vi.stubGlobal("fetch", fetch);
  render(
    createElement(
      ArtistPage,
      page([], [recording("first", "First verse")], 2, true),
    ),
  );
  fireEvent.click(screen.getByRole("button", { name: "Load more" }));
  await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  await waitFor(() => expect(screen.getByText("Second verse")).toBeTruthy());
});
