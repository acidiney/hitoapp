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
    recently_added: [],
    unheard: [],
    ...home,
  },
  artists: { items: [], total: 0, offset: 0, limit: 12 },
});
const albums = (count: number) =>
  Array.from({ length: count }, (_, index) => ({
    id: `album-${index}`,
    title: `Album ${index}`,
    artist: "Shore",
    inferred_artist: null,
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
    inferred_artist: null,
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

test("home greets the display name and falls back to the username", () => {
  const data = props({});
  data.user = {
    id: "alice",
    username: "Alice",
    admin: true,
    display_name: "Ana",
  };
  const { unmount } = render(createElement(HomePage, data));
  expect(screen.getByRole("heading", { level: 1 }).textContent).toMatch(/Ana$/);
  unmount();
  data.user = {
    id: "alice",
    username: "Alice",
    admin: true,
    display_name: null,
  };
  render(createElement(HomePage, data));
  expect(screen.getByRole("heading", { level: 1 }).textContent).toMatch(
    /Alice$/,
  );
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

const group = (id: string, name: string): Schema<"ArtistIdentityGroup"> => ({
  id,
  provider: { kind: "builtin_musicbrainz" },
  provider_artist_id: id,
  name: {
    artist_id: id,
    association_id: id,
    recording_id: `r-${id}`,
    value: name,
  },
  library_count: 1,
  recording_count: 4,
  release_count: 1,
});

test("home shows recently played, recently added and chosen artists as shelves", () => {
  const data = props({
    recent_albums: albums(2),
    recently_added: [
      { id: "new-1", title: "Fresh", artist: "Shore", inferred_artist: null },
    ],
  });
  data.artists = {
    items: [group("g1", "Slipknot"), group("g2", "Shore")],
    total: 2,
    offset: 0,
    limit: 12,
  };
  render(createElement(HomePage, data));
  const played = screen.getByRole("region", { name: "Recently played" });
  expect(
    within(played)
      .getAllByRole("link")
      .map((link) => link.getAttribute("href")),
  ).toEqual(["/albums/album-0", "/albums/album-1"]);
  const added = screen.getByRole("region", { name: "Recently added" });
  expect(
    within(added).getByRole("link", { name: /Fresh/ }).getAttribute("href"),
  ).toBe("/albums/new-1");
  const artists = screen.getByRole("region", { name: "Your artists" });
  expect(
    within(artists)
      .getAllByRole("link", { name: /Slipknot|Shore/ })
      .map((link) => link.getAttribute("href")),
  ).toEqual(["/artist-groups/g1", "/artist-groups/g2"]);
  expect(within(artists).getAllByText("4 tracks")).toBeTruthy();
  expect(
    screen
      .getByRole("link", { name: "Discover music from your services" })
      .getAttribute("href"),
  ).toBe("/discover");
});

test("shelves with nothing to show are left out", () => {
  render(createElement(HomePage, props({ liked_count: 2 })));
  expect(screen.queryByRole("region", { name: "Recently played" })).toBeNull();
  expect(screen.queryByRole("region", { name: "Recently added" })).toBeNull();
  expect(screen.queryByRole("region", { name: "Your artists" })).toBeNull();
});
