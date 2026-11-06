import { createElement, type AnchorHTMLAttributes } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ArtistIndexPage from "../pages/ArtistIndexPage";
import { artistIdentityBrowsing } from "../fixtures/artistIdentityBrowsing";

vi.mock("@inertiajs/react", () => ({
  router: { get: vi.fn(), reload: vi.fn() },
  usePage: () => ({ url: "/artists" }),
  useRemember: (initial: unknown) => [initial, vi.fn()],
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

const QUEUE = "/api/v1/artist-matching/review-queue?limit=50";
const DISMISSED = "/api/v1/artist-matching/review-queue/dismissed?limit=50";
const STATUS = "/api/v1/artist-matching/status";

function empty() {
  return Response.json({ items: [], total: 0, offset: 0, limit: 50 });
}

function serve(queue: unknown[]) {
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string) => {
      if (path === QUEUE)
        return Response.json({
          items: queue,
          total: queue.length,
          offset: 0,
          limit: 50,
        });
      if (path === DISMISSED) return empty();
      if (path === STATUS)
        return Response.json({
          enabled: true,
          state: "idle",
          matched_so_far: 0,
          last_finished_at: Date.now() / 1000 - 60,
          last_matched: 4,
        });
      throw new Error(`Unexpected ${path}`);
    }),
  );
}

const credit = {
  artist_id: "credit-1",
  name: "Nova Quartet",
  undecided_tracks: 2,
  needs_search: false,
  search_recording_id: null,
  candidates: [
    {
      provider_artist_id: "44444444-4444-4444-8444-444444444444",
      name: "Nova Quartet",
      kind: "group",
      disambiguation: "Lisbon fado band",
      country: "PT",
      corroborated_titles: 1,
      in_library: false,
      origin: "name_search",
    },
  ],
};

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test("one plain status line, the review button, the artists and no empty chosen panel", async () => {
  serve([credit]);
  const { index } = artistIdentityBrowsing();
  index.identity_groups.items = [];
  index.identity_groups.total = 0;
  render(createElement(ArtistIndexPage, index));
  await screen.findByRole("button", { name: "1 artist needs review" });
  const status = await screen.findByText(/Matching finished · 4 matched/);
  expect(status.textContent).not.toMatch(/MusicBrainz|Looks up/);
  expect(screen.queryByText(/No chosen artists yet/)).toBeNull();
  const grid = screen.getByRole("region", { name: "Artists in your library" });
  const chosen = screen.queryByRole("region", { name: "Chosen artists" });
  expect(grid).toBeTruthy();
  expect(chosen).toBeNull();
});

test("artist cards use one count format and leave out zero parts", () => {
  serve([]);
  const { index } = artistIdentityBrowsing();
  index.artists.items = [
    {
      ...index.artists.items[0],
      id: "a",
      name: "Both",
      recording_count: 4,
      release_count: 2,
    },
    {
      ...index.artists.items[0],
      id: "b",
      name: "Albums only",
      recording_count: 0,
      release_count: 1,
    },
    {
      ...index.artists.items[0],
      id: "c",
      name: "One each",
      recording_count: 1,
      release_count: 1,
    },
  ];
  render(createElement(ArtistIndexPage, index));
  const grid = screen.getByRole("region", { name: "Artists in your library" });
  const text = within(grid)
    .getAllByRole("link")
    .map((link) => link.querySelector("small")?.textContent);
  expect(text).toEqual(["4 tracks · 2 albums", "1 album", "1 track · 1 album"]);
});

test("pending reviews open in a dialog and the confirmation list is not on the page", async () => {
  serve([credit]);
  const { index } = artistIdentityBrowsing();
  render(createElement(ArtistIndexPage, index));
  expect(screen.queryByRole("dialog")).toBeNull();
  const button = await screen.findByRole("button", {
    name: "1 artist needs review",
  });
  expect(screen.queryByText("Needs your confirmation (1)")).toBeNull();
  button.focus();
  fireEvent.click(button);
  const dialog = await screen.findByRole("dialog", {
    name: "Artists to review",
  });
  expect(
    within(dialog).getByText("Nova Quartet", { selector: "strong" }),
  ).toBeTruthy();
  fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(document.activeElement).toBe(button);
});

test("with nothing to review there is no review button", async () => {
  serve([]);
  const { index } = artistIdentityBrowsing();
  render(createElement(ArtistIndexPage, index));
  await screen.findByText(/Matching finished/);
  expect(screen.queryByRole("button", { name: /need.* review/ })).toBeNull();
});
