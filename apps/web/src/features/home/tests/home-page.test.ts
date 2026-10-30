import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import HomePage from "../pages/HomePage";

const { player, router } = vi.hoisted(() => ({
  player: { playQueue: vi.fn().mockResolvedValue(undefined) },
  router: { get: vi.fn() },
}));
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => player,
}));
vi.mock("@inertiajs/react", () => ({
  router,
  Link: ({ href, children, ...rest }: { href: string; children: ReactNode }) =>
    createElement("a", { href, ...rest }, children),
}));

const track = (id: string, title: string): Schema<"Recording"> => ({
  id,
  title,
  artist: "Shore",
  inherited_title: title,
  inherited_artist: "Shore",
  library_id: "library",
  album: "Tide",
  release_id: "tide",
  duration_ms: 1000,
  available: true,
  plays_from_services: false,
  liked: false,
});
const props = (
  home: Partial<Schema<"HomePageProps">["home"]> = {},
): Schema<"HomePageProps"> => ({
  errors: {},
  native_playback: false,
  lock: { pin_enabled: false, idle_minutes: 0, locked: false },
  user: { id: "alice", username: "Alice", admin: true },
  libraries: [],
  playlists: [],
  home: {
    has_music: true,
    liked_count: 0,
    recent_albums: [],
    unheard: [],
    ...home,
  },
});
const albums = (count: number) =>
  Array.from({ length: count }, (_, index) => ({
    id: `album-${index}`,
    title: `Album ${index}`,
    artist: "Shore",
    last_played_at: 100 - index,
    played: 2,
  }));

test("the greeting names the account and follows the time of day", () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date(2026, 5, 1, 15, 30));
  render(createElement(HomePage, props()));
  expect(
    screen.getByRole("heading", { level: 1, name: "Good afternoon, Alice" }),
  ).toBeTruthy();
});

test("the search field sends the query to the Search page", () => {
  render(createElement(HomePage, props()));
  const field = screen.getByRole("searchbox", { name: "Search" });
  expect(field.id).toBe("shell-search");
  fireEvent.change(field, { target: { value: "tide" } });
  fireEvent.submit(field.closest("form") as HTMLFormElement);
  expect(router.get).toHaveBeenCalledWith("/search", { q: "tide" });
});

test("quick access shows only real listening: recent albums, likes and playlists, at most six", () => {
  const data = props({ liked_count: 611, recent_albums: albums(4) });
  data.playlists = [
    { id: "p1", name: "Sunday calm", kind: "local" },
    { id: "p2", name: "Fado", kind: "followed" },
    { id: "p3", name: "Extra", kind: "local" },
  ];
  render(createElement(HomePage, data));
  const quick = screen.getByRole("region", { name: "Quick access" });
  const links = within(quick).getAllByRole("link");
  expect(links).toHaveLength(6);
  expect(links.map((link) => link.getAttribute("href"))).toEqual([
    "/albums/album-0",
    "/albums/album-1",
    "/albums/album-2",
    "/likes",
    "/playlists/p1",
    "/playlists/p2",
  ]);
  expect(within(quick).getByText("611 tracks")).toBeTruthy();
  expect(within(quick).getByText("Followed playlist")).toBeTruthy();
});

test("nothing is invented for sections the server cannot fill", () => {
  render(createElement(HomePage, props({ recent_albums: albums(1) })));
  expect(screen.queryByText(/New from your artists/)).toBeNull();
  expect(
    screen.queryByRole("region", { name: "Unheard, near what you like" }),
  ).toBeNull();
});

test("unheard tracks explain why they are offered and play on request", () => {
  const unheard = {
    release_id: "tide",
    title: "Tide",
    artist: "Shore",
    played: 3,
    remaining: 4,
    tracks: [track("t2", "Tide two"), track("t3", "Tide three")],
  };
  render(
    createElement(
      HomePage,
      props({ recent_albums: albums(1), unheard: [unheard] }),
    ),
  );
  const section = screen.getByRole("region", {
    name: "Unheard, near what you like",
  });
  expect(
    within(section).getByText("You played 3 tracks from Tide."),
  ).toBeTruthy();
  expect(within(section).getByText("4 more tracks from Tide")).toBeTruthy();
  fireEvent.click(
    within(section).getByRole("button", { name: "Listen to Tide" }),
  );
  expect(player.playQueue).toHaveBeenCalledWith(unheard.tracks);
  expect(
    within(section)
      .getByRole("link", { name: "Open Tide" })
      .getAttribute("href"),
  ).toBe("/albums/tide");
});

test("an empty library offers folders, playlist import and services", () => {
  const data = props({ has_music: false });
  render(createElement(HomePage, data));
  expect(screen.getByText("Your library is still empty")).toBeTruthy();
  expect(
    screen
      .getByRole("link", { name: "Choose a music folder" })
      .getAttribute("href"),
  ).toBe("/settings?section=music");
  expect(
    screen
      .getByRole("link", { name: "Import a playlist" })
      .getAttribute("href"),
  ).toBe("/playlists");
  expect(
    screen
      .getByRole("link", { name: "Connect a service" })
      .getAttribute("href"),
  ).toBe("/settings?section=plugins");
  expect(screen.queryByRole("region", { name: "Quick access" })).toBeNull();
});

test("a listener who cannot manage folders is told to ask the administrator", () => {
  const data = props({ has_music: false });
  data.user = { id: "bob", username: "Bob", admin: false };
  render(createElement(HomePage, data));
  expect(
    screen.queryByRole("link", { name: "Choose a music folder" }),
  ).toBeNull();
  expect(screen.getByText(/Ask your administrator/)).toBeTruthy();
});
