import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { NoticeProvider } from "../../../shared/ui/Notices";
import GenrePage from "../pages/GenrePage";

const { player } = vi.hoisted(() => ({
  player: {
    playQueue: vi.fn().mockResolvedValue(undefined),
    subscribe: () => () => undefined,
    snapshot: () => ({ current: null, shuffle: false }),
    shuffle: vi.fn(),
  },
}));
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => player,
}));
vi.mock("@inertiajs/react", () => ({
  Link: ({ href, children, ...rest }: { href: string; children: ReactNode }) =>
    createElement("a", { href, ...rest }, children),
}));

const track = (id: string, title: string): Schema<"Recording"> => ({
  id,
  title,
  artist: "Ana",
  inherited_title: title,
  inherited_artist: "Ana",
  library_id: "library",
  album: "Album X",
  release_id: "x",
  duration_ms: 61000,
  available: true,
  plays_from_services: false,
  liked: false,
});
const props = (count: number): Schema<"GenrePageProps"> => ({
  errors: {},
  native_playback: false,
  lock: { pin_enabled: false, idle_minutes: 0, locked: false },
  user: { id: "alice", username: "Alice", admin: false },
  libraries: [],
  playlists: [],
  genre: {
    genre: { key: "fado", label: "Fado", track_count: count },
    albums: [
      {
        id: "x",
        library_id: "library",
        title: "Album X",
        artist: "Ana",
        artist_id: "ana",
        track_count: 2,
        duration_ms: 120000,
        artwork_recording_id: "t1",
        inferred_artist: null,
      },
    ],
    tracks: [track("t1", "Song 1"), track("t2", "Song 2")],
  },
});

test("a lowercase MusicBrainz genre label is shown in title case in the heading and breadcrumb", () => {
  const base = props(2);
  const lower: Schema<"GenrePageProps"> = {
    ...base,
    genre: {
      ...base.genre,
      genre: { key: "pop soul", label: "pop soul", track_count: 2 },
    },
  };
  render(createElement(NoticeProvider, null, createElement(GenrePage, lower)));
  expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
    "Pop Soul",
  );
  expect(screen.getAllByText("Pop Soul").length).toBeGreaterThan(1);
  expect(screen.queryByText("pop soul")).toBeNull();
});

test("a genre page lists its albums and tracks and plays or shuffles them", () => {
  render(
    createElement(NoticeProvider, null, createElement(GenrePage, props(2))),
  );
  expect(screen.getByRole("heading", { level: 1, name: "Fado" })).toBeTruthy();
  expect(screen.getByText("2 tracks in your library")).toBeTruthy();
  expect(
    within(screen.getByRole("navigation", { name: "Breadcrumb" }))
      .getByRole("link", { name: "Discover" })
      .getAttribute("href"),
  ).toBe("/discover");
  expect(
    screen
      .getByRole("link", { name: "Open Album X by Ana" })
      .getAttribute("href"),
  ).toBe("/albums/x");
  expect(screen.getByText("Song 1")).toBeTruthy();
  expect(screen.queryByText(/Showing/)).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Play Fado tracks" }));
  expect(player.playQueue).toHaveBeenCalledTimes(1);
  expect(
    player.playQueue.mock.calls[0][0].map((t: { id: string }) => t.id),
  ).toEqual(["t1", "t2"]);
  expect(
    screen.getByRole("button", { name: "Shuffle Fado tracks" }),
  ).toBeTruthy();
});

test("a long genre says how many tracks the page shows", () => {
  render(
    createElement(NoticeProvider, null, createElement(GenrePage, props(142))),
  );
  expect(screen.getByText("Showing 2 of 142 tracks.")).toBeTruthy();
});

test("a genre at the server's count cap reads 1,000+", () => {
  render(
    createElement(NoticeProvider, null, createElement(GenrePage, props(1000))),
  );
  expect(screen.getByText("1,000+ tracks in your library")).toBeTruthy();
  expect(screen.getByText("Showing 2 of 1,000+ tracks.")).toBeTruthy();
});
