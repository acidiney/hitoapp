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
  vi.stubGlobal("fetch", fetch);
  render(createElement(DiscoverPage, props()));
  const section = screen.getByRole("region", { name: "By genre" });
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
  expect(screen.queryByRole("region", { name: "By genre" })).toBeNull();
  expect(screen.queryByRole("region", { name: "Radio from…" })).toBeNull();
  expect(
    screen.getByRole("link", { name: "Connect a music service" }),
  ).toBeTruthy();
});

test("radio starts from one chosen artist with one request and plays the returned queue", async () => {
  const fetch = vi.fn().mockResolvedValue(
    new Response(
      JSON.stringify({
        artist: "Ana Moura",
        similar_artists: 2,
        tracks: [track("t1", "One"), track("t2", "Two")],
      }),
    ),
  );
  vi.stubGlobal("fetch", fetch);
  render(createElement(DiscoverPage, props()));
  const section = screen.getByRole("region", { name: "Radio from…" });
  expect(within(section).getByText("12 tracks in your library")).toBeTruthy();
  fireEvent.click(
    within(section).getByRole("button", { name: "Start radio from Ana Moura" }),
  );
  await waitFor(() => expect(player.playQueue).toHaveBeenCalledTimes(1));
  expect(fetch).toHaveBeenCalledTimes(1);
  const [url, init] = fetch.mock.calls[0];
  expect(url).toBe("/api/v1/artist-identity-groups/g1/radio");
  expect(init.method).toBe("POST");
  expect(
    player.playQueue.mock.calls[0][0].map((t: { id: string }) => t.id),
  ).toEqual(["t1", "t2"]);
  expect(await screen.findByRole("status")).toHaveProperty(
    "textContent",
    "Playing a mix of Ana Moura and 2 similar artists from your library.",
  );
});

test("a radio with no playable tracks or a failed request says so and keeps the page", async () => {
  const fetch = vi
    .fn()
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
  vi.stubGlobal("fetch", fetch);
  render(createElement(DiscoverPage, props()));
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
  expect(screen.getByRole("region", { name: "By genre" })).toBeTruthy();
});
