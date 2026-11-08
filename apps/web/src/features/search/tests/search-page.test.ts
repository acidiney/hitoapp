import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { expect, test, vi } from "vitest";
import SearchPage from "../pages/SearchPage";

const { player, router } = vi.hoisted(() => ({
  player: {
    playQueue: vi.fn().mockResolvedValue(undefined),
    prepareSelection: vi.fn(() => vi.fn().mockResolvedValue(true)),
    subscribe: () => () => {},
    snapshot: () => ({ current: null }),
  },
  router: { get: vi.fn() },
}));
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => player,
}));
vi.mock("../../../shared/hooks/useMutation", () => ({
  useMutation: () => ({
    mutate: vi.fn(async () => ({ ok: true })),
    pending: false,
  }),
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
): Schema<"Recording"> => ({
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
  plays_from_services: false,
});
const connection: Schema<"PluginInstallation"> = {
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
const SETTINGS = "/api/v1/metadata/settings";
const SEARCH = "/api/v1/plugins/connection/search";

function stubServices(
  options: {
    automatic?: boolean | "unreadable";
    items?: unknown[];
    status?: number;
  } = {},
) {
  const searches: { query: string }[] = [];
  const fetch = vi.fn(async (path: string, init?: RequestInit) => {
    if (path === SETTINGS) {
      if (options.automatic === "unreadable")
        return new Response("{}", { status: 500 });
      return Response.json({
        revision: 1,
        enabled: true,
        providers: [],
        automatic_artist_lookup: true,
        automatic_artwork: true,
        automatic_service_search: options.automatic ?? true,
      });
    }
    expect(path).toBe(SEARCH);
    searches.push(JSON.parse(init?.body as string));
    return Response.json(
      options.status && options.status !== 200
        ? { error: "The service is unavailable." }
        : { items: options.items ?? [] },
      { status: options.status ?? 200 },
    );
  });
  vi.stubGlobal("fetch", fetch);
  return searches;
}
const settled = () => new Promise((resolve) => setTimeout(resolve, 30));

const page = <T>(items: T[]) => ({
  items,
  total: items.length,
  offset: 0,
  limit: 8,
});
const props = (
  over: Partial<Schema<"SearchPageProps">> = {},
): Schema<"SearchPageProps"> => ({
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
      inferred_artist: null,
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
  artist_groups: page([]),
  connections: [],
  service_names: {},
  ...over,
});

test("library results are grouped by kind with availability and one action each", () => {
  render(createElement(SearchPage, props()));
  const tracks = screen.getByRole("region", { name: "Tracks" });
  expect(within(tracks).getByText("Not playable yet")).toBeTruthy();
  fireEvent.click(
    within(tracks).getByRole("button", { name: "Play Tide one" }),
  );
  expect(player.playQueue).toHaveBeenCalled();
  expect(
    within(tracks).getByRole("link", { name: "Tide two" }).getAttribute("href"),
  ).toBe("/recordings/r2");
  expect(
    within(tracks).getByRole("button", { name: "Actions for Tide two" }),
  ).toBeTruthy();
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
  stubServices();
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

test("a plain search sends nothing to a provider until the listener asks", async () => {
  const hit = { id: "remote", title: "Provider tide", artist: "Remote" };
  const searches = stubServices({ items: [hit] });
  render(
    createElement(
      SearchPage,
      props({ connections: [connection], service_names: { service: "Home" } }),
    ),
  );
  await settled();
  expect(searches).toEqual([]);
  expect(screen.getByRole("region", { name: "Tracks" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Search your services" }));
  expect(
    await screen.findByRole("button", { name: "Play Provider tide" }),
  ).toBeTruthy();
  expect(searches.map((body) => body.query)).toEqual(["tide"]);
});

test("provider results use the connected service with explicit listen and add actions", async () => {
  const hit = {
    id: "remote",
    title: "Provider tide",
    artist: "Remote",
    duration_ms: 1000,
  };
  const searches = stubServices({ items: [hit] });
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
  expect(searches.map((body) => body.query)).toEqual(["tide"]);
});

test("a provider outage warns inside its section and keeps library results", async () => {
  stubServices({ status: 502 });
  render(
    createElement(
      SearchPage,
      props({ connections: [connection], service_names: { service: "Home" } }),
    ),
  );
  fireEvent.click(screen.getByRole("button", { name: "Search your services" }));
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

test("the best match is listed once, in its own section, with that section first", () => {
  render(createElement(SearchPage, props()));
  expect(screen.queryByRole("heading", { name: "Top result" })).toBeNull();
  expect(screen.getAllByText("Tide one")).toHaveLength(1);
  const order = screen
    .getAllByRole("heading", { level: 2 })
    .map((heading) => heading.textContent);
  expect(order.slice(0, 3)).toEqual(["Tracks", "Albums", "Artists"]);
  cleanup();
  render(
    createElement(
      SearchPage,
      props({
        query: "shore",
        artist_groups: page([group("g-shore", "Shore")]),
      }),
    ),
  );
  const exact = screen
    .getAllByRole("heading", { level: 2 })
    .map((heading) => heading.textContent);
  expect(exact.slice(0, 3)).toEqual(["Artists", "Tracks", "Albums"]);
});

test("every section says how many results it has and when it shows only some", () => {
  render(
    createElement(
      SearchPage,
      props({
        recordings: {
          items: [
            recording("r1", "Tide one", true),
            recording("r2", "Tide two", false),
          ],
          total: 24,
          offset: 0,
          limit: 8,
        },
      }),
    ),
  );
  expect(
    within(screen.getByRole("region", { name: "Tracks" })).getByText(
      "Showing 2 of 24",
    ),
  ).toBeTruthy();
  expect(
    within(screen.getByRole("region", { name: "Albums" })).getByText(
      "1 result",
    ),
  ).toBeTruthy();
  expect(
    within(screen.getByRole("region", { name: "Artists" })).getByText(
      "1 result",
    ),
  ).toBeTruthy();
});

test("no results suggests checking the spelling and offers to search the connected services", async () => {
  const empty = { items: [], total: 0, offset: 0, limit: 8 };
  const searches = stubServices({ automatic: false, items: [] });
  render(
    createElement(
      SearchPage,
      props({
        query: "zzz",
        recordings: empty,
        releases: empty,
        artists: empty,
        connections: [connection],
        service_names: { service: "Home" },
      }),
    ),
  );
  expect(
    screen.getByText("Nothing in your library matches “zzz”"),
  ).toBeTruthy();
  expect(screen.getByText(/Check the spelling/)).toBeTruthy();
  await settled();
  expect(searches).toEqual([]);
  const buttons = screen.getAllByRole("button", {
    name: "Search your services",
  });
  expect(buttons).toHaveLength(1);
  fireEvent.click(buttons[0]);
  await settled();
  expect(searches.map((body) => body.query)).toEqual(["zzz"]);
});

test("no results without a connected service points to Settings", () => {
  const empty = { items: [], total: 0, offset: 0, limit: 8 };
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
  expect(screen.getByText(/Check the spelling/)).toBeTruthy();
  expect(
    screen
      .getByRole("link", { name: "Connect a music service" })
      .getAttribute("href"),
  ).toBe("/settings?section=plugins");
});

test("the Library scope with no results offers the services without sending anything itself", async () => {
  const empty = { items: [], total: 0, offset: 0, limit: 8 };
  const searches = stubServices({ automatic: false, items: [] });
  render(
    createElement(
      SearchPage,
      props({
        query: "zzz",
        recordings: empty,
        releases: empty,
        artists: empty,
        connections: [connection],
        service_names: { service: "Home" },
      }),
    ),
  );
  fireEvent.click(screen.getByRole("button", { name: "Library" }));
  await settled();
  expect(searches).toEqual([]);
  fireEvent.click(screen.getByRole("button", { name: "Search your services" }));
  await settled();
  expect(searches.map((body) => body.query)).toEqual(["zzz"]);
  expect(
    screen.getByRole("button", { name: "All" }).getAttribute("aria-pressed"),
  ).toBe("true");
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
  expect(
    screen.getByText("Nothing in your library matches “zzz”"),
  ).toBeTruthy();
});

function phoneScreen() {
  vi.stubGlobal(
    "matchMedia",
    vi.fn((query: string) => ({
      matches: query === "(max-width: 767px)",
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })),
  );
}

test("on a phone the page owns the field and submitting it searches again", () => {
  phoneScreen();
  render(createElement(SearchPage, props()));
  const field = screen.getByRole("searchbox", { name: "Search" });
  expect(field.id).toBe("shell-search");
  expect((field as HTMLInputElement).value).toBe("tide");
  fireEvent.change(field, { target: { value: "glass" } });
  fireEvent.submit(field.closest("form") as HTMLFormElement);
  expect(router.get).toHaveBeenCalledWith("/search", { q: "glass" });
});

test("on a desktop the field lives in the top bar, so the page adds none", () => {
  render(createElement(SearchPage, props()));
  expect(screen.queryByRole("searchbox")).toBeNull();
});

const group = (id: string, name: string): Schema<"ArtistIdentityGroup"> => ({
  id,
  name: { value: name, recording_id: "r1" } as never,
  provider: { kind: "builtin_musicbrainz" },
  provider_artist_id: `mb-${id}`,
  recording_count: 4,
  release_count: 2,
  library_count: 1,
});

test("chosen artists come from the server in its order and an exact match comes first", () => {
  render(
    createElement(
      SearchPage,
      props({
        query: "shore",
        artist_groups: page([
          group("g-shore", "Shore"),
          group("g-band", "Shore Band"),
        ]),
        artists: page([]),
      }),
    ),
  );
  expect(screen.queryByRole("heading", { name: "Top result" })).toBeNull();
  const artists = screen.getByRole("region", { name: "Artists" });
  expect(
    within(artists)
      .getAllByRole("link")
      .map((link) => link.getAttribute("href")),
  ).toEqual(["/artist-groups/g-shore", "/artist-groups/g-band"]);
});

test("library credits follow the chosen artists without repeating a chosen name", () => {
  render(
    createElement(
      SearchPage,
      props({
        query: "shore",
        artist_groups: page([group("g-shore", "Shore")]),
        artists: page([
          {
            id: "shore",
            library_id: "library",
            name: "Shore",
            recording_count: 3,
            release_count: 1,
          },
          {
            id: "shore-band",
            library_id: "library",
            name: "Shore Band",
            recording_count: 1,
            release_count: 1,
          },
        ]),
      }),
    ),
  );
  const artists = screen.getByRole("region", { name: "Artists" });
  expect(
    within(artists)
      .getAllByRole("link")
      .map((link) => link.getAttribute("href")),
  ).toEqual(["/artist-groups/g-shore", "/artists/shore-band"]);
});

test("few library results search the connected services without another click", async () => {
  const hit = { id: "remote", title: "Provider joe", artist: "Remote" };
  const searches = stubServices({ items: [hit] });
  const empty = { items: [], total: 0, offset: 0, limit: 8 };
  render(
    createElement(
      SearchPage,
      props({
        query: "joe",
        recordings: empty,
        releases: empty,
        artists: page([
          {
            id: "joe",
            library_id: "library",
            name: "Joe",
            recording_count: 1,
            release_count: 1,
          },
        ]),
        connections: [connection],
        service_names: { service: "Home" },
      }),
    ),
  );
  expect(
    await screen.findByRole("button", { name: "Play Provider joe" }),
  ).toBeTruthy();
  expect(searches.map((body) => body.query)).toEqual(["joe"]);
  expect(
    screen.getByText(/query is sent to your connected services/i),
  ).toBeTruthy();
  expect(
    screen
      .getAllByRole("link", { name: "Change in Settings" })
      .every(
        (link) => link.getAttribute("href") === "/settings?section=privacy",
      ),
  ).toBe(true);
  expect(
    screen.queryByRole("button", { name: "Search your services" }),
  ).toBeNull();
});

const fewResults = () => {
  const empty = { items: [], total: 0, offset: 0, limit: 8 };
  return props({
    query: "joe",
    recordings: empty,
    releases: empty,
    artists: empty,
    connections: [connection],
    service_names: { service: "Home" },
  });
};

test("the server turning automatic service search off restores the explicit button", async () => {
  const searches = stubServices({ automatic: false });
  render(createElement(SearchPage, fewResults()));
  await settled();
  expect(searches).toEqual([]);
  expect(
    screen.getByRole("button", { name: "Search your services" }),
  ).toBeTruthy();
  expect(screen.queryByText(/query is sent to your connected/i)).toBeNull();
});

test("the Library scope never sends the query to a service", async () => {
  const searches = stubServices();
  const connected = {
    connections: [connection],
    service_names: { service: "Home" },
  };
  const view = render(createElement(SearchPage, props(connected)));
  fireEvent.click(screen.getByRole("button", { name: "Library" }));
  const empty = { items: [], total: 0, offset: 0, limit: 8 };
  view.rerender(
    createElement(
      SearchPage,
      props({
        ...connected,
        query: "joe",
        recordings: empty,
        releases: empty,
        artists: empty,
      }),
    ),
  );
  await settled();
  expect(searches).toEqual([]);
});

test("when the choice cannot be read automatic service search stays off", async () => {
  const searches = stubServices({ automatic: "unreadable" });
  render(createElement(SearchPage, fewResults()));
  await settled();
  expect(searches).toEqual([]);
  expect(
    screen.getByRole("button", { name: "Search your services" }),
  ).toBeTruthy();
});

test("nothing is sent before the choice has been read", async () => {
  const searches: unknown[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn((path: string) => {
      if (path !== SETTINGS) searches.push(path);
      return new Promise<Response>(() => {});
    }),
  );
  render(createElement(SearchPage, fewResults()));
  await settled();
  expect(searches).toEqual([]);
});

test("the disclosure sits next to the search field before any result", async () => {
  stubServices();
  render(
    createElement(
      SearchPage,
      props({ query: "", connections: [connection], service_names: {} }),
    ),
  );
  expect(
    await screen.findByText(
      /few matches in your library, your search is sent to your connected services/i,
    ),
  ).toBeTruthy();
});

test("album results without an album artist show the server's inferred credit", () => {
  render(
    createElement(
      SearchPage,
      props({
        releases: page([
          {
            id: "mix",
            library_id: "library",
            title: "Mix",
            artist: null,
            artist_id: null,
            duration_ms: 1000,
            track_count: 3,
            inferred_artist: { kind: "various" },
          },
          {
            id: "solo",
            library_id: "library",
            title: "Solo",
            artist: null,
            artist_id: null,
            duration_ms: 1000,
            track_count: 3,
            inferred_artist: { kind: "named", name: "Shore" },
          },
        ]),
      }),
    ),
  );
  const albums = screen.getByRole("region", { name: "Albums" });
  expect(within(albums).getByText(/^Various artists · 3 tracks/)).toBeTruthy();
  expect(within(albums).getByText(/^Shore · 3 tracks/)).toBeTruthy();
  expect(within(albums).queryByText(/Unknown artist/)).toBeNull();
});
