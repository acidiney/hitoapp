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
      year: null,
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
  expect(within(tracks).getByText("No source")).toBeTruthy();
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

const connected = {
  connections: [connection],
  service_names: { service: "Home" },
};
const SERVICE = /Home · Fixture service/;
const emptyPage = { items: [], total: 0, offset: 0, limit: 8 };
const nothing = (query: string) =>
  props({
    query,
    recordings: emptyPage,
    releases: emptyPage,
    artists: emptyPage,
  });

test("scope pills narrow the results to one kind", () => {
  render(createElement(SearchPage, props()));
  const scope = screen.getByRole("group", { name: "Search scope" });
  expect(
    within(scope)
      .getAllByRole("button")
      .map((b) => b.textContent),
  ).toEqual(["All", "Tracks", "Albums", "Artists"]);
  expect(
    within(scope)
      .getByRole("button", { name: "All" })
      .getAttribute("aria-pressed"),
  ).toBe("true");
  fireEvent.click(within(scope).getByRole("button", { name: "Albums" }));
  expect(screen.queryByRole("region", { name: "Tracks" })).toBeNull();
  expect(screen.getByRole("region", { name: "Albums" })).toBeTruthy();
  fireEvent.click(within(scope).getByRole("button", { name: "Tracks" }));
  expect(screen.getByRole("region", { name: "Tracks" })).toBeTruthy();
  expect(screen.queryByRole("region", { name: "Albums" })).toBeNull();
});

test("a plain search sends nothing to a service until the listener turns it on", async () => {
  const hit = { id: "remote", title: "Provider tide", artist: "Remote" };
  const searches = stubServices({ items: [hit] });
  render(createElement(SearchPage, props(connected)));
  await settled();
  expect(searches).toEqual([]);
  const toggle = screen.getByRole("button", { name: SERVICE });
  expect(toggle.getAttribute("aria-pressed")).toBe("false");
  fireEvent.click(toggle);
  expect(
    await screen.findByRole("button", { name: "Play Provider tide" }),
  ).toBeTruthy();
  expect(toggle.getAttribute("aria-pressed")).toBe("true");
  expect(searches.map((body) => body.query)).toEqual(["tide"]);
  fireEvent.click(toggle);
  expect(toggle.getAttribute("aria-pressed")).toBe("false");
  expect(
    screen.queryByRole("button", { name: "Play Provider tide" }),
  ).toBeNull();
});

test("service results name the service and offer explicit listen and add actions", async () => {
  const hit = {
    id: "remote",
    title: "Provider tide",
    artist: "Remote",
    duration_ms: 1000,
  };
  stubServices({ items: [hit] });
  render(createElement(SearchPage, props(connected)));
  fireEvent.click(screen.getByRole("button", { name: SERVICE }));
  expect(
    await screen.findByRole("button", { name: "Play Provider tide" }),
  ).toBeTruthy();
  expect(
    screen.getByRole("button", { name: "Add Provider tide to library" }),
  ).toBeTruthy();
  expect(screen.getByRole("heading", { name: "From Home" })).toBeTruthy();
  expect(screen.getByText(/Remote · Home · not in your library/)).toBeTruthy();
  expect(
    screen
      .getByRole("link", { name: "Change what gets sent" })
      .getAttribute("href"),
  ).toBe("/settings?section=privacy");
});

test("a service outage warns inside its section and keeps library results", async () => {
  stubServices({ status: 502 });
  render(createElement(SearchPage, props(connected)));
  fireEvent.click(screen.getByRole("button", { name: SERVICE }));
  expect((await screen.findByRole("alert")).textContent).toContain(
    "unavailable",
  );
  expect(screen.getByRole("region", { name: "Tracks" })).toBeTruthy();
});

test("without a connected service the library is the only place searched", () => {
  render(createElement(SearchPage, props()));
  const sources = screen.getByRole("group", { name: "Searching in" });
  expect(within(sources).getByText("Your library")).toBeTruthy();
  expect(within(sources).queryAllByRole("button")).toHaveLength(0);
});

test("the best match leads in a top result and is not repeated as a section", () => {
  render(createElement(SearchPage, props()));
  const top = screen.getByRole("region", { name: "Top result" });
  expect(within(top).getByRole("link", { name: "Tide one" })).toBeTruthy();
  fireEvent.click(within(top).getByRole("button", { name: "Play" }));
  expect(player.playQueue).toHaveBeenCalled();
  expect(
    screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent),
  ).toEqual(["Top result", "Tracks", "Albums", "Artists"]);
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
  const exact = screen.getByRole("region", { name: "Top result" });
  expect(
    within(exact).getByRole("link", { name: "Shore" }).getAttribute("href"),
  ).toBe("/artist-groups/g-shore");
  expect(
    screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent),
  ).toEqual(["Top result", "Tracks", "Albums"]);
});

test("the tracks section offers every match when it shows only some", () => {
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
  fireEvent.click(screen.getByRole("button", { name: "Show all 24" }));
  expect(
    screen.getByRole("button", { name: "Tracks" }).getAttribute("aria-pressed"),
  ).toBe("true");
  expect(screen.queryByRole("button", { name: "Show all 24" })).toBeNull();
});

test("no results suggests checking the spelling and offers to search the connected service", async () => {
  const searches = stubServices({ automatic: false, items: [] });
  render(createElement(SearchPage, { ...nothing("zzz"), ...connected }));
  expect(
    screen.getByText("Nothing in your library matches “zzz”"),
  ).toBeTruthy();
  expect(screen.getByText(/Check the spelling/)).toBeTruthy();
  await settled();
  expect(searches).toEqual([]);
  const offer = screen.getByRole("button", { name: "Search Home" });
  fireEvent.click(offer);
  await settled();
  expect(searches.map((body) => body.query)).toEqual(["zzz"]);
  expect(screen.queryByRole("button", { name: "Search Home" })).toBeNull();
});

test("no results without a connected service points to Settings", () => {
  render(createElement(SearchPage, nothing("zzz")));
  expect(screen.getByText(/Check the spelling/)).toBeTruthy();
  expect(
    screen
      .getByRole("link", { name: "Connect a music service" })
      .getAttribute("href"),
  ).toBe("/settings?section=plugins");
});

test("an empty query asks for a search and no match says so", () => {
  const { unmount } = render(
    createElement(SearchPage, { ...nothing(""), query: "" }),
  );
  expect(
    screen.getByText("Search your library and connected services"),
  ).toBeTruthy();
  unmount();
  render(createElement(SearchPage, nothing("zzz")));
  expect(
    screen.getByText("Nothing in your library matches “zzz”"),
  ).toBeTruthy();
});

test("the field has the focus target id and submitting searches at once", () => {
  render(createElement(SearchPage, props()));
  const field = screen.getByRole("searchbox", { name: "Search" });
  expect(field.id).toBe("shell-search");
  expect((field as HTMLInputElement).value).toBe("tide");
  fireEvent.change(field, { target: { value: "glass" } });
  fireEvent.submit(field.closest("form") as HTMLFormElement);
  expect(router.get).toHaveBeenCalledWith(
    "/search",
    { q: "glass" },
    expect.objectContaining({ replace: false }),
  );
});

test("typing updates the results after a short pause without adding history", () => {
  vi.useFakeTimers();
  try {
    router.get.mockClear();
    render(createElement(SearchPage, props()));
    const field = screen.getByRole("searchbox", { name: "Search" });
    fireEvent.change(field, { target: { value: "gl" } });
    fireEvent.change(field, { target: { value: "glass" } });
    expect(router.get).not.toHaveBeenCalled();
    vi.advanceTimersByTime(400);
    expect(router.get).toHaveBeenCalledTimes(1);
    expect(router.get).toHaveBeenCalledWith(
      "/search",
      { q: "glass" },
      expect.objectContaining({ replace: true, preserveState: true }),
    );
  } finally {
    vi.useRealTimers();
  }
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

test("an exact artist match leads and the other artists follow in the server's order", () => {
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
  const top = screen.getByRole("region", { name: "Top result" });
  expect(
    within(top).getByRole("link", { name: "Shore" }).getAttribute("href"),
  ).toBe("/artist-groups/g-shore");
  const artists = screen.getByRole("region", { name: "Artists" });
  expect(
    within(artists)
      .getAllByRole("link")
      .map((link) => link.getAttribute("href")),
  ).toEqual(["/artist-groups/g-band"]);
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
  ).toEqual(["/artists/shore-band"]);
});

const fewResults = () => ({
  ...nothing("joe"),
  ...connected,
  artists: page([
    {
      id: "joe",
      library_id: "library",
      name: "Joe",
      recording_count: 1,
      release_count: 1,
    },
  ]),
});

test("few library results search the connected service without another click", async () => {
  const hit = { id: "remote", title: "Provider joe", artist: "Remote" };
  const searches = stubServices({ items: [hit] });
  render(createElement(SearchPage, fewResults()));
  expect(
    await screen.findByRole("button", { name: "Play Provider joe" }),
  ).toBeTruthy();
  expect(searches.map((body) => body.query)).toEqual(["joe"]);
  expect(
    screen.getByText(/because your library has few matches/i),
  ).toBeTruthy();
  expect(
    screen
      .getAllByRole("link", { name: /Change (in Settings|what gets sent)/ })
      .every(
        (link) => link.getAttribute("href") === "/settings?section=privacy",
      ),
  ).toBe(true);
  expect(
    screen.getByRole("button", { name: SERVICE }).getAttribute("aria-pressed"),
  ).toBe("true");
});

test("the server turning automatic service search off leaves the service off", async () => {
  const searches = stubServices({ automatic: false });
  render(createElement(SearchPage, fewResults()));
  await settled();
  expect(searches).toEqual([]);
  expect(
    screen.getByRole("button", { name: SERVICE }).getAttribute("aria-pressed"),
  ).toBe("false");
  expect(
    screen.queryByText(/because your library has few matches/i),
  ).toBeNull();
});

test("the Albums and Artists scopes never send the query to a service", async () => {
  const searches = stubServices();
  const view = render(createElement(SearchPage, props(connected)));
  fireEvent.click(screen.getByRole("button", { name: "Albums" }));
  view.rerender(createElement(SearchPage, fewResults()));
  await settled();
  expect(searches).toEqual([]);
});

test("when the choice cannot be read automatic service search stays off", async () => {
  const searches = stubServices({ automatic: "unreadable" });
  render(createElement(SearchPage, fewResults()));
  await settled();
  expect(searches).toEqual([]);
  expect(
    screen.getByRole("button", { name: SERVICE }).getAttribute("aria-pressed"),
  ).toBe("false");
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
            year: null,
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
            year: null,
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

test("a 300-character artist credit stays in its card with an ellipsis, a title and no zero counts", () => {
  const name = `Pineapple ${"Collaborator ".repeat(24)}`.trim();
  render(
    createElement(
      SearchPage,
      props({
        query: "pineapple",
        recordings: { items: [], total: 0, offset: 0, limit: 8 },
        releases: { items: [], total: 0, offset: 0, limit: 8 },
        artists: page([
          {
            id: "first",
            library_id: "library",
            name: "Pineapple",
            recording_count: 2,
            release_count: 1,
          },
          {
            id: "long",
            library_id: "library",
            name,
            recording_count: 1,
            release_count: 0,
          },
        ]),
      }),
    ),
  );
  expect(name.length).toBeGreaterThan(290);
  const card = within(
    screen.getByRole("region", { name: "Artists" }),
  ).getByRole("link");
  const title = card.querySelector("strong")!;
  expect(title.getAttribute("title")).toBe(name);
  const copy = card.querySelector(".search-card-copy")!;
  expect(copy.textContent).toContain("1 track");
  expect(copy.textContent).not.toMatch(/0 albums/);
  expect(card.closest(".search-cards")).toBeTruthy();
});

test("album links in the top result and album cards encode the album id", () => {
  render(
    createElement(
      SearchPage,
      props({
        recordings: emptyPage,
        artists: emptyPage,
        releases: page([
          {
            id: "a/b c",
            library_id: "library",
            title: "Tide",
            artist: "Shore",
            artist_id: "shore",
            duration_ms: 1000,
            track_count: 3,
            inferred_artist: null,
            year: null,
          },
        ]),
      }),
    ),
  );
  const top = screen.getByRole("region", { name: "Top result" });
  expect(
    within(top)
      .getAllByRole("link")
      .map((link) => link.getAttribute("href")),
  ).toEqual(["/albums/a%2Fb%20c", "/albums/a%2Fb%20c"]);
  const albums = screen.getByRole("region", { name: "Albums" });
  expect(
    within(albums).getByRole("link", { name: /Tide/ }).getAttribute("href"),
  ).toBe("/albums/a%2Fb%20c");
});
