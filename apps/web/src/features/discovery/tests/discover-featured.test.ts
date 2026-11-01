import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import DiscoverPage from "../pages/DiscoverPage";

const { player, router } = vi.hoisted(() => ({
  player: {
    prepareSelection: vi.fn(() => vi.fn().mockResolvedValue(true)),
    playQueue: vi.fn().mockResolvedValue(undefined),
  },
  router: { visit: vi.fn() },
}));
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => player,
}));
vi.mock("@inertiajs/react", () => ({
  router,
  Link: ({ href, children, ...rest }: { href: string; children: ReactNode }) =>
    createElement("a", { href, ...rest }, children),
}));

const props: Schema<"DiscoverPageProps"> = {
  errors: {},
  native_playback: false,
  lock: { pin_enabled: false, idle_minutes: 0, locked: false },
  user: { id: "alice", username: "Alice", admin: false },
  libraries: [],
  playlists: [],
  connections: [],
  service_names: {},
  genres: [],
  artists: { items: [], total: 0, offset: 0, limit: 6 },
};

const entry = (
  id: string,
  extra: Partial<Schema<"FeaturedEntry">> = {},
): Schema<"FeaturedEntry"> => ({
  installation_id: "install-1",
  installation_revision: 4,
  provider: "Owned service",
  id,
  title: `Featured ${id}`,
  subtitle: `By ${id}`,
  kind: "playlist",
  reference: `https://provider.example/playlist/${id}`,
  artwork_id: `art-${id}`,
  item_count: 12,
  existing_playlist_id: null,
  ...extra,
});

const track = (id: string, available = true): Schema<"PlaylistEntry"> => ({
  entry_id: `entry-${id}`,
  recording_id: `rec-${id}`,
  position: 0,
  title: `Song ${id}`,
  inherited_title: `Song ${id}`,
  artist: "Artist",
  inherited_artist: "Artist",
  duration_ms: 1000,
  liked: false,
  available,
  plays_from_services: true,
  origin: null,
});

function stub(
  shelf: Schema<"FeaturedShelf"> | Response | Error,
  entries: Schema<"PlaylistEntry">[] = [track("1"), track("2")],
  importStatus = 200,
) {
  const calls: { url: string; method: string; body: unknown }[] = [];
  const fetch = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    const body = init?.body ? JSON.parse(String(init.body)) : null;
    calls.push({ url, method: init?.method ?? "GET", body });
    if (url === "/api/v1/discover/featured") {
      if (shelf instanceof Error) throw shelf;
      return shelf instanceof Response ? shelf : Response.json(shelf);
    }
    if (url === "/api/v1/plugins/install-1/playlists/open")
      return importStatus === 200
        ? Response.json({
            request_id: body.request_id,
            playlist_id: "pl-1",
            reused: false,
          })
        : Response.json(
            { error: "The provider is unavailable" },
            { status: importStatus },
          );
    if (url === "/api/v1/playlists/pl-1")
      return Response.json({
        playlist: { id: "pl-1", name: "Copy", kind: "local" },
        origin: null,
        entries,
      });
    return new Response("{}", { status: 404 });
  });
  vi.stubGlobal("fetch", fetch);
  return calls;
}

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  player.playQueue.mockClear();
  router.visit.mockClear();
});

const shelfOf = (...items: Schema<"FeaturedEntry">[]) =>
  ({ status: "available", items }) as Schema<"FeaturedShelf">;

test("the shelf lists provider items with kind, count and a cover from the approved connection", async () => {
  stub(
    shelfOf(
      entry("a"),
      entry("b", {
        kind: "new_release",
        artwork_id: null,
        item_count: 1,
        subtitle: null,
      }),
    ),
  );
  const { container } = render(createElement(DiscoverPage, props));
  const region = await screen.findByRole("region", {
    name: "From your services",
  });
  const cards = within(region).getAllByRole("listitem");
  expect(cards).toHaveLength(2);
  expect(cards[0].textContent).toContain("Featured a");
  expect(cards[0].textContent).toContain("By a");
  expect(cards[0].textContent).toContain("Playlist · 12 tracks");
  expect(cards[1].textContent).toContain("New release · 1 track");
  const covers = container.querySelectorAll(".featured-card img");
  expect(covers).toHaveLength(1);
  expect(covers[0].getAttribute("src")).toBe(
    "/api/v1/plugins/install-1/featured-artwork?revision=4&artwork=art-a",
  );
  expect(within(cards[1]).getByRole("img", { name: /Cover of/ })).toBeTruthy();
});

test.each([
  ["unsupported", { status: "unsupported" } as Schema<"FeaturedShelf">],
  ["disabled", { status: "disabled" } as Schema<"FeaturedShelf">],
  ["unavailable", { status: "unavailable" } as Schema<"FeaturedShelf">],
  ["empty", shelfOf()],
  ["failed", new Response("{}", { status: 500 })],
  ["offline", new TypeError("network down")],
])("a %s shelf is hidden without any error wall", async (_name, shelf) => {
  const calls = stub(shelf);
  render(createElement(DiscoverPage, props));
  await waitFor(() =>
    expect(calls.some((call) => call.url === "/api/v1/discover/featured")).toBe(
      true,
    ),
  );
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(
    screen.queryByRole("region", { name: "From your services" }),
  ).toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();
  expect(
    screen.getByRole("heading", { name: "Search your services" }),
  ).toBeTruthy();
});

test("Play on a card imports a copy, queues its recordings and keeps the page", async () => {
  const calls = stub(shelfOf(entry("a")), [
    track("1"),
    { ...track("x"), recording_id: null },
    track("2", false),
  ]);
  render(createElement(DiscoverPage, props));
  fireEvent.click(
    await screen.findByRole("button", { name: "Play Featured a" }),
  );
  await waitFor(() => expect(player.playQueue).toHaveBeenCalledTimes(1));
  const queued = player.playQueue.mock.calls[0][0] as { id: string }[];
  expect(queued.map((item) => item.id)).toEqual(["rec-1", "rec-2"]);
  const imported = calls.find((call) => call.url.endsWith("/playlists/open"))!;
  expect(imported.method).toBe("POST");
  expect(imported.body).toEqual({
    request_id: expect.stringMatching(/^[0-9a-f-]{36}$/),
    installation_revision: 4,
    playlist_reference: "https://provider.example/playlist/a",
  });
  expect(router.visit).not.toHaveBeenCalled();
  const status = await screen.findByRole("status");
  expect(status.textContent).toContain("Playing Featured a");
  expect(status.textContent).toContain("Playlists");
});

test("Open on a card imports a copy and shows its playlist page", async () => {
  stub(shelfOf(entry("a")));
  render(createElement(DiscoverPage, props));
  fireEvent.click(
    await screen.findByRole("button", { name: "Open Featured a" }),
  );
  await waitFor(() =>
    expect(router.visit).toHaveBeenCalledWith("/playlists/pl-1"),
  );
  expect(player.playQueue).not.toHaveBeenCalled();
});

test("a failed import says so beside the shelf and leaves every card usable", async () => {
  const calls = stub(shelfOf(entry("a"), entry("b")), undefined, 502);
  render(createElement(DiscoverPage, props));
  fireEvent.click(
    await screen.findByRole("button", { name: "Play Featured a" }),
  );
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("provider is unavailable");
  expect(player.playQueue).not.toHaveBeenCalled();
  expect(
    (
      screen.getByRole("button", {
        name: "Play Featured b",
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(false);
  expect(calls.filter((call) => call.url.endsWith("/open"))).toHaveLength(1);
});

test("a copy with nothing playable reports it instead of playing", async () => {
  stub(shelfOf(entry("a")), [{ ...track("x"), recording_id: null }]);
  render(createElement(DiscoverPage, props));
  fireEvent.click(
    await screen.findByRole("button", { name: "Play Featured a" }),
  );
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("Nothing playable");
  expect(player.playQueue).not.toHaveBeenCalled();
});

test("only one card imports at a time", async () => {
  const calls = stub(shelfOf(entry("a"), entry("b")));
  render(createElement(DiscoverPage, props));
  const first = await screen.findByRole("button", { name: "Play Featured a" });
  fireEvent.click(first);
  fireEvent.click(first);
  fireEvent.click(screen.getByRole("button", { name: "Open Featured b" }));
  await waitFor(() => expect(player.playQueue).toHaveBeenCalled());
  expect(calls.filter((call) => call.url.endsWith("/open"))).toHaveLength(1);
});

test("leaving the page cancels the shelf request", async () => {
  let signal: AbortSignal | undefined;
  vi.stubGlobal(
    "fetch",
    vi.fn((_input: RequestInfo | URL, init?: RequestInit) => {
      signal = init?.signal ?? undefined;
      return new Promise<Response>(() => undefined);
    }),
  );
  const view = render(createElement(DiscoverPage, props));
  await waitFor(() => expect(signal).toBeDefined());
  view.unmount();
  expect(signal?.aborted).toBe(true);
});

test("a card that already has a copy skips the import and a second action reuses the first", async () => {
  const calls = stub(
    shelfOf(entry("a", { existing_playlist_id: "pl-1" }), entry("b")),
  );
  render(createElement(DiscoverPage, props));
  fireEvent.click(
    await screen.findByRole("button", { name: "Play Featured a" }),
  );
  await waitFor(() => expect(player.playQueue).toHaveBeenCalledTimes(1));
  expect(calls.some((call) => call.url.endsWith("/open"))).toBe(false);

  fireEvent.click(screen.getByRole("button", { name: "Play Featured b" }));
  await waitFor(() => expect(player.playQueue).toHaveBeenCalledTimes(2));
  fireEvent.click(screen.getByRole("button", { name: "Open Featured b" }));
  await waitFor(() =>
    expect(router.visit).toHaveBeenCalledWith("/playlists/pl-1"),
  );
  expect(calls.filter((call) => call.url.endsWith("/open"))).toHaveLength(1);
});

test("a timed out import says a copy may already exist", async () => {
  stub(shelfOf(entry("a")));
  const original = globalThis.fetch;
  vi.stubGlobal("fetch", (input: RequestInfo | URL, init?: RequestInit) =>
    String(input).endsWith("/open")
      ? Promise.reject(new DOMException("aborted", "AbortError"))
      : original(input, init),
  );
  render(createElement(DiscoverPage, props));
  fireEvent.click(
    await screen.findByRole("button", { name: "Play Featured a" }),
  );
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("may already be in your Playlists");
});

test("the shelf empties when the account changes and the new answer has nothing", async () => {
  stub(shelfOf(entry("a")));
  const view = render(createElement(DiscoverPage, props));
  await screen.findByRole("region", { name: "From your services" });
  stub({ status: "unsupported" });
  view.rerender(
    createElement(DiscoverPage, {
      ...props,
      user: { id: "bob", username: "Bob", admin: false },
    }),
  );
  await waitFor(() =>
    expect(
      screen.queryByRole("region", { name: "From your services" }),
    ).toBeNull(),
  );
});

test("a cover that fails to load is asked for once more before the generated cover stays", async () => {
  stub(shelfOf(entry("a")));
  vi.useFakeTimers({ toFake: ["setTimeout"], shouldAdvanceTime: true });
  try {
    const { container } = render(createElement(DiscoverPage, props));
    await screen.findByRole("region", { name: "From your services" });
    fireEvent.error(container.querySelector(".featured-card img")!);
    expect(container.querySelector(".featured-card img")).toBeNull();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1500);
    });
    const retry = container.querySelector(".featured-card img");
    expect(retry).not.toBeNull();
    fireEvent.error(retry!);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(container.querySelector(".featured-card img")).toBeNull();
  } finally {
    vi.useRealTimers();
  }
});

test("the page says that playing a card adds a playlist copy", async () => {
  stub({ status: "unsupported" });
  render(createElement(DiscoverPage, props));
  expect(screen.getByText(/adds a copy to your Playlists/)).toBeTruthy();
});
