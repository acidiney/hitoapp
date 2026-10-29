import { createElement, type ReactNode } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import SearchPage from "../pages/SearchPage";
import type {
  PluginInstallation,
  Recording,
  SearchPageProps,
} from "../../../shared/types";

const { player, router } = vi.hoisted(() => ({
  player: {
    playQueue: vi.fn().mockResolvedValue(undefined),
    prepareSelection: vi.fn(() => vi.fn().mockResolvedValue(true)),
  },
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

const recording = (
  id: string,
  title: string,
  available: boolean,
): Recording => ({
  id,
  title,
  artist: "Shore",
  inherited_title: title,
  inherited_artist: "Shore",
  library_id: "library",
  album: "Tide",
  release_id: "tide",
  duration_ms: 63000,
  available,
  liked: false,
});
const connection: PluginInstallation = {
  id: "connection",
  service_id: "service",
  revision: 1,
  checked_at: 1,
  credential_set: false,
  manifest: {
    id: "org.hito.fixture",
    name: "Fixture service",
    protocol_version: 1,
    capabilities: ["search", "sources"],
  },
};
const page = <T>(items: T[]) => ({
  items,
  total: items.length,
  offset: 0,
  limit: 8,
});
const props = (over: Partial<SearchPageProps> = {}): SearchPageProps => ({
  errors: {},
  native_playback: false,
  lock: { pin_enabled: false, idle_minutes: 0, locked: false },
  user: { id: "alice", username: "Alice", admin: false },
  libraries: [],
  playlists: [],
  query: "tide",
  recordings: page([
    recording("r1", "Tide one", true),
    recording("r2", "Tide two", false),
  ]),
  releases: page([
    {
      id: "tide",
      library_id: "library",
      title: "Tide",
      artist: "Shore",
      artist_id: "shore",
      duration_ms: 1000,
      track_count: 3,
    },
  ]),
  artists: page([
    {
      id: "shore",
      library_id: "library",
      name: "Shore",
      recording_count: 3,
      release_count: 1,
    },
  ]),
  connections: [],
  service_names: {},
  ...over,
});

test("library results are grouped with a top result, availability and one action each", () => {
  render(createElement(SearchPage, props()));
  expect(screen.getByRole("heading", { name: "Top result" })).toBeTruthy();
  const tracks = screen.getByRole("region", { name: "Tracks" });
  expect(within(tracks).getByText("In library")).toBeTruthy();
  expect(within(tracks).getByText("Catalog only")).toBeTruthy();
  fireEvent.click(
    within(tracks).getByRole("button", { name: "Play Tide one" }),
  );
  expect(player.playQueue).toHaveBeenCalled();
  expect(
    within(tracks)
      .getByRole("link", { name: "View Tide two" })
      .getAttribute("href"),
  ).toBe("/recordings/r2");
  const albums = screen.getByRole("region", { name: "Albums" });
  expect(
    within(albums).getByRole("link", { name: /Tide/ }).getAttribute("href"),
  ).toBe("/albums/tide");
  const artists = screen.getByRole("region", { name: "Artists" });
  expect(
    within(artists).getByRole("link", { name: /Shore/ }).getAttribute("href"),
  ).toBe("/artists/shore");
});

test("scope pills narrow the results to the library or to providers", () => {
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue(new Response(JSON.stringify({ items: [] }))),
  );
  render(
    createElement(
      SearchPage,
      props({ connections: [connection], service_names: { service: "Home" } }),
    ),
  );
  const scope = screen.getByRole("group", { name: "Search scope" });
  expect(
    within(scope)
      .getAllByRole("button")
      .map((b) => b.textContent),
  ).toEqual(["All", "Library", "Providers"]);
  expect(
    within(scope)
      .getByRole("button", { name: "All" })
      .getAttribute("aria-pressed"),
  ).toBe("true");
  fireEvent.click(within(scope).getByRole("button", { name: "Providers" }));
  expect(screen.queryByRole("region", { name: "Tracks" })).toBeNull();
  fireEvent.click(within(scope).getByRole("button", { name: "Library" }));
  expect(screen.getByRole("region", { name: "Tracks" })).toBeTruthy();
});

test("provider results use the connected service with explicit listen and add actions", async () => {
  const hit = {
    id: "remote",
    title: "Provider tide",
    artist: "Remote",
    duration_ms: 1000,
  };
  const fetch = vi
    .fn()
    .mockResolvedValue(new Response(JSON.stringify({ items: [hit] })));
  vi.stubGlobal("fetch", fetch);
  render(
    createElement(
      SearchPage,
      props({ connections: [connection], service_names: { service: "Home" } }),
    ),
  );
  fireEvent.click(screen.getByRole("button", { name: "Providers" }));
  expect(
    await screen.findByRole("button", { name: "Play Provider tide" }),
  ).toBeTruthy();
  expect(
    screen.getByRole("button", { name: "Add Provider tide to library" }),
  ).toBeTruthy();
  expect(fetch.mock.calls[0][0]).toBe("/api/v1/plugins/connection/search");
  expect(JSON.parse(fetch.mock.calls[0][1].body).query).toBe("tide");
});

test("a provider outage warns inside its section and keeps library results", async () => {
  vi.stubGlobal(
    "fetch",
    vi
      .fn()
      .mockResolvedValue(
        new Response(JSON.stringify({ error: "The service is unavailable." }), {
          status: 502,
        }),
      ),
  );
  render(
    createElement(
      SearchPage,
      props({ connections: [connection], service_names: { service: "Home" } }),
    ),
  );
  expect((await screen.findByRole("alert")).textContent).toContain(
    "unavailable",
  );
  expect(screen.getByRole("region", { name: "Tracks" })).toBeTruthy();
});

test("providers scope without a connected service points to Settings", () => {
  render(createElement(SearchPage, props()));
  fireEvent.click(screen.getByRole("button", { name: "Providers" }));
  expect(
    screen
      .getByRole("link", { name: "Connect a music service" })
      .getAttribute("href"),
  ).toBe("/settings?section=plugins");
});

test("an empty query asks for a search and no match says so", () => {
  const empty = { items: [], total: 0, offset: 0, limit: 8 };
  const { unmount } = render(
    createElement(
      SearchPage,
      props({ query: "", recordings: empty, releases: empty, artists: empty }),
    ),
  );
  expect(
    screen.getByText("Search your library and connected services"),
  ).toBeTruthy();
  unmount();
  render(
    createElement(
      SearchPage,
      props({
        query: "zzz",
        recordings: empty,
        releases: empty,
        artists: empty,
      }),
    ),
  );
  expect(screen.getByText("No matches in your library")).toBeTruthy();
});

test("submitting the field searches again", () => {
  render(createElement(SearchPage, props()));
  const field = screen.getByRole("searchbox", { name: "Search" });
  expect((field as HTMLInputElement).value).toBe("tide");
  fireEvent.change(field, { target: { value: "glass" } });
  fireEvent.submit(field.closest("form") as HTMLFormElement);
  expect(router.get).toHaveBeenCalledWith("/search", { q: "glass" });
});
