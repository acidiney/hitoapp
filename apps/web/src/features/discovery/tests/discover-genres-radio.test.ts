import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import DiscoverPage from "../pages/DiscoverPage";
import { skippingFeatured } from "../fixtures/skippingFeatured";

const { player } = vi.hoisted(() => ({
  player: {
    prepareSelection: vi.fn(() => vi.fn().mockResolvedValue(true)),
    playQueue: vi.fn().mockResolvedValue(undefined),
  },
}));
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => player,
}));
vi.mock("@inertiajs/react", () => ({
  Link: ({ href, children, ...rest }: { href: string; children: ReactNode }) =>
    createElement("a", { href, ...rest }, children),
}));

const group = (id: string, name: string, tracks: number) =>
  ({
    id,
    provider: { kind: "builtin_musicbrainz" },
    provider_artist_id: `${id}-mbid`,
    name: {
      value: name,
      recording_id: `${id}-recording`,
      artist_id: `${id}-credit`,
      association_id: `${id}-association`,
    },
    library_count: 1,
    recording_count: tracks,
    release_count: 1,
  }) as Schema<"ArtistIdentityGroup">;
const props = (
  extra: Partial<Schema<"DiscoverPageProps">> = {},
): Schema<"DiscoverPageProps"> => ({
  errors: {},
  native_playback: false,
  lock: { pin_enabled: false, idle_minutes: 0, locked: false },
  user: { id: "alice", username: "Alice", admin: false },
  libraries: [],
  playlists: [],
  connections: [],
  service_names: {},
  genres: [
    { key: "fado", label: "Fado", track_count: 3 },
    { key: "hip hop", label: "hip hop", track_count: 1 },
    { key: "contemporary r&b", label: "contemporary r&b", track_count: 2 },
  ],
  artists: {
    items: [group("g1", "Ana Moura", 12), group("g2", "Bruma", 1)],
    total: 2,
    offset: 0,
    limit: 6,
  },
  ...extra,
});
const track = (id: string, title: string) => ({
  id,
  title,
  artist: "Ana Moura",
  inherited_title: title,
  inherited_artist: "Ana Moura",
  library_id: "library",
  album: "Album",
  release_id: "release",
  duration_ms: 1000,
  available: true,
  plays_from_services: false,
  liked: false,
});

afterEach(() => {
  vi.unstubAllGlobals();
  player.playQueue.mockClear();
});

test("genre tiles come from the library and open their genre page without any request", () => {
  const fetch = vi.fn();
  vi.stubGlobal("fetch", skippingFeatured(fetch));
  render(
    createElement(
      DiscoverPage,
      props({ artists: { items: [], total: 0, offset: 0, limit: 6 } }),
    ),
  );
  const section = screen.getByRole("region", { name: "Genres you own" });
  const links = within(section).getAllByRole("link");
  expect(links.map((link) => link.textContent)).toEqual([
    "Fado3 tracks",
    "Hip Hop1 track",
    "Contemporary R&B2 tracks",
  ]);
  expect(links.map((link) => link.getAttribute("href"))).toEqual([
    "/genres?genre=fado",
    "/genres?genre=hip%20hop",
    "/genres?genre=contemporary%20r%26b",
  ]);
  expect(fetch).not.toHaveBeenCalled();
});

test("genre tiles are flat calm colours chosen by position and no inline colour", () => {
  vi.stubGlobal("fetch", skippingFeatured(vi.fn()));
  render(
    createElement(
      DiscoverPage,
      props({ artists: { items: [], total: 0, offset: 0, limit: 6 } }),
    ),
  );
  const tiles = within(
    screen.getByRole("region", { name: "Genres you own" }),
  ).getAllByRole("link");
  expect(tiles.map((tile) => tile.getAttribute("data-tone"))).toEqual([
    "0",
    "1",
    "2",
  ]);
  for (const tile of tiles) expect(tile.getAttribute("style")).toBeNull();
});

test("only twelve genres show at first and Show all reveals the rest", () => {
  vi.stubGlobal("fetch", skippingFeatured(vi.fn()));
  const many = Array.from({ length: 14 }, (_, index) => ({
    key: `g${index}`,
    label: `Genre ${index}`,
    track_count: 1,
  }));
  render(
    createElement(
      DiscoverPage,
      props({
        genres: many,
        artists: { items: [], total: 0, offset: 0, limit: 6 },
      }),
    ),
  );
  const section = screen.getByRole("region", { name: "Genres you own" });
  expect(within(section).getAllByRole("link")).toHaveLength(12);
  fireEvent.click(within(section).getByRole("button", { name: "Show all 14" }));
  expect(within(section).getAllByRole("link")).toHaveLength(14);
  fireEvent.click(within(section).getByRole("button", { name: "Show fewer" }));
  expect(within(section).getAllByRole("link")).toHaveLength(12);
});

test("sections with nothing to show are left out and the provider search stays", () => {
  render(
    createElement(
      DiscoverPage,
      props({
        genres: [],
        artists: { items: [], total: 0, offset: 0, limit: 6 },
      }),
    ),
  );
  expect(screen.queryByRole("region", { name: "Genres you own" })).toBeNull();
  expect(screen.queryByRole("region", { name: "Suggested radio" })).toBeNull();
  expect(
    screen.queryByRole("region", { name: "Start a radio from an artist" }),
  ).toBeNull();
  expect(
    screen.getByRole("link", { name: "Connect a music service" }),
  ).toBeTruthy();
});

const radioFetch = (reply: (group: string) => Response) =>
  vi.fn(async (path: string) => {
    const group = /artist-identity-groups\/([^/]+)\/radio/.exec(path)?.[1];
    return group ? reply(group) : new Response("{}", { status: 404 });
  });

test("the most owned artist leads with a radio preview of what plays first", async () => {
  const fetch = radioFetch(
    () =>
      new Response(
        JSON.stringify({
          artist: "Ana Moura",
          similar_artists: 2,
          tracks: [track("t1", "One"), track("t2", "Two")],
        }),
      ),
  );
  vi.stubGlobal("fetch", skippingFeatured(fetch));
  render(createElement(DiscoverPage, props()));
  const hero = screen.getByRole("region", { name: "Suggested radio" });
  expect(
    within(hero).getByRole("heading", { name: "Ana Moura radio" }),
  ).toBeTruthy();
  expect(await within(hero).findByText("One")).toBeTruthy();
  expect(within(hero).getByText("Two")).toBeTruthy();
  expect(
    within(hero).getByText(
      "Starts with 2 ready tracks, then 2 nearby artists in your library.",
    ),
  ).toBeTruthy();
  fireEvent.click(within(hero).getByRole("button", { name: "Start radio" }));
  await waitFor(() => expect(player.playQueue).toHaveBeenCalledTimes(1));
  expect(
    player.playQueue.mock.calls[0][0].map((t: { id: string }) => t.id),
  ).toEqual(["t1", "t2"]);
});

test("radio starts from one chosen artist with one request and plays the returned queue", async () => {
  const fetch = radioFetch(
    (group) =>
      new Response(
        JSON.stringify({
          artist: group === "g2" ? "Bruma" : "Ana Moura",
          similar_artists: 2,
          tracks: [track("t1", "One"), track("t2", "Two")],
        }),
      ),
  );
  vi.stubGlobal("fetch", skippingFeatured(fetch));
  render(createElement(DiscoverPage, props()));
  const section = screen.getByRole("region", {
    name: "Start a radio from an artist",
  });
  expect(within(section).getByText("12 tracks in your library")).toBeTruthy();
  const before = fetch.mock.calls.length;
  fireEvent.click(
    within(section).getByRole("button", { name: "Start radio from Bruma" }),
  );
  await waitFor(() => expect(player.playQueue).toHaveBeenCalledTimes(1));
  const started = fetch.mock.calls
    .slice(before)
    .filter(([path]) => String(path).includes("/g2/radio"));
  expect(started).toHaveLength(1);
  const [url, init] = started[0];
  expect(url).toBe("/api/v1/artist-identity-groups/g2/radio");
  expect(init.method).toBe("POST");
  expect(
    player.playQueue.mock.calls[0][0].map((t: { id: string }) => t.id),
  ).toEqual(["t1", "t2"]);
  expect(await screen.findByRole("status")).toHaveProperty(
    "textContent",
    "Playing a mix of Bruma and 2 similar artists from your library.",
  );
});

test("a radio with no playable tracks or a failed request says so and keeps the page", async () => {
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(
      new Response(
        JSON.stringify({ artist: "Ana Moura", similar_artists: 0, tracks: [] }),
      ),
    )
    .mockResolvedValueOnce(
      new Response(
        JSON.stringify({ artist: "Bruma", similar_artists: 0, tracks: [] }),
      ),
    )
    .mockResolvedValueOnce(
      new Response(JSON.stringify({ code: "internal", message: "Broken" }), {
        status: 500,
      }),
    );
  vi.stubGlobal("fetch", skippingFeatured(fetch));
  render(createElement(DiscoverPage, props()));
  await waitFor(() => expect(fetch).toHaveBeenCalledTimes(1));
  fireEvent.click(
    screen.getByRole("button", { name: "Start radio from Bruma" }),
  );
  expect((await screen.findByRole("alert")).textContent).toContain(
    "Nothing playable from Bruma is in your library right now.",
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Start radio from Ana Moura" }),
  );
  await waitFor(() =>
    expect(screen.getByRole("alert").textContent).toContain(
      "Hito returned 500",
    ),
  );
  expect(player.playQueue).not.toHaveBeenCalled();
  expect(screen.getByRole("region", { name: "Genres you own" })).toBeTruthy();
});
