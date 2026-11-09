import type { Schema } from "../../../shared/schema";
import { createElement, type AnchorHTMLAttributes } from "react";
import { cleanup, render, screen } from "@testing-library/react";
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
  usePage: () => ({ url: window.location.pathname }),
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

afterEach(cleanup);

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
