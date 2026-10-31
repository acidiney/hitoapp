import { createElement, type AnchorHTMLAttributes } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ArtistCreditIdentity from "../components/ArtistCreditIdentity";
import type { Schema } from "../../../shared/schema";

vi.mock("@inertiajs/react", () => ({
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
  router: { reload: vi.fn() },
}));

const account = "10000000-0000-4000-8000-000000000001";
const credit = { id: "credit-ed", name: "Ed Sheeran" };
const groupId = "70000000-0000-4000-8000-000000000009";
const suggestionsPath = `/api/v1/artists/${credit.id}/match-suggestions`;
const statusPath = "/api/v1/artist-matching/status";

const chosenGroup: Schema<"ArtistIdentityGroup"> = {
  id: groupId,
  provider: { kind: "builtin_musicbrainz" },
  provider_artist_id: "b8a7c51f-362c-4dcb-a259-bc6e0095f0a6",
  name: {
    value: "Ed Sheeran",
    recording_id: "r1",
    artist_id: "a1",
    association_id: "s1",
  },
  library_count: 1,
  recording_count: 106,
  release_count: 7,
};

function suggestions(
  overrides: Partial<Schema<"ArtistMatchSuggestions">> = {},
): Schema<"ArtistMatchSuggestions"> {
  return {
    artist_id: credit.id,
    chosen: [],
    undecided_tracks: 0,
    dismissed: 0,
    dismissed_items: [],
    items: [],
    ...overrides,
  };
}

function status(
  state: Schema<"ArtistMatchingStatus">["state"],
): Schema<"ArtistMatchingStatus"> {
  return {
    enabled: true,
    state,
    matched_so_far: 0,
    last_finished_at: null,
    last_matched: 0,
  };
}

function serve(
  read: Schema<"ArtistMatchSuggestions">,
  matching: Schema<"ArtistMatchingStatus">,
) {
  const paths: string[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string) => {
      paths.push(path);
      if (path === suggestionsPath) return Response.json(read);
      if (path === statusPath) return Response.json(matching);
      throw new Error("Unexpected request");
    }),
  );
  return paths;
}

afterEach(() => vi.unstubAllGlobals());

test("a credit matched automatically names its artist and offers no search", async () => {
  const paths = serve(
    suggestions({
      chosen: [{ group: chosenGroup, tracks: 106, automatic_tracks: 106 }],
    }),
    status("idle"),
  );
  render(createElement(ArtistCreditIdentity, { account, artist: credit }));
  expect(
    await screen.findByText(/Artist: Ed Sheeran · matched automatically/),
  ).toBeTruthy();
  const open = screen.getByRole("link", { name: "Open artist" });
  expect(open.getAttribute("href")).toBe(`/artist-groups/${groupId}`);
  expect(
    screen.queryByRole("button", { name: "Choose the artist" }),
  ).toBeNull();
  expect(screen.queryByText(/Find reviewed artists/)).toBeNull();
  expect(paths.some((path) => path.includes("artist-identity-groups"))).toBe(
    false,
  );
});

test("while the server walk is running the page says it is identifying the artist", async () => {
  serve(suggestions({ undecided_tracks: 106 }), status("running"));
  render(createElement(ArtistCreditIdentity, { account, artist: credit }));
  expect(
    await screen.findByText("Hito is identifying this artist…"),
  ).toBeTruthy();
  expect(screen.queryByText("Not identified yet")).toBeNull();
});

test("an uncertain credit says so and keeps the manual choice behind a button", async () => {
  const paths = serve(suggestions({ undecided_tracks: 106 }), status("idle"));
  render(createElement(ArtistCreditIdentity, { account, artist: credit }));
  expect(await screen.findByText("Not identified yet")).toBeTruthy();
  expect(screen.queryByRole("group", { name: "Choose the artist" })).toBeNull();
  expect(paths.some((path) => path.includes("artist-identity-groups"))).toBe(
    false,
  );
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string) => {
      if (path.startsWith("/api/v1/artist-identity-groups"))
        return Response.json({ items: [], total: 0, offset: 0, limit: 20 });
      throw new Error("Unexpected request");
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Choose the artist" }));
  expect(
    await screen.findByRole("group", { name: "Choose the artist" }),
  ).toBeTruthy();
  expect(
    await screen.findByText(/No reviewed artist matches this credit yet/),
  ).toBeTruthy();
});

test("a credit whose tracks map to several chosen artists lists each with its count", async () => {
  const other: Schema<"ArtistIdentityGroup"> = {
    ...chosenGroup,
    id: "70000000-0000-4000-8000-00000000000a",
    provider_artist_id: "c9b8d62f-473d-4ecb-b36a-cd7f1106a1b7",
  };
  serve(
    suggestions({
      chosen: [
        { group: chosenGroup, tracks: 80, automatic_tracks: 80 },
        { group: other, tracks: 26, automatic_tracks: 0 },
      ],
    }),
    status("idle"),
  );
  render(createElement(ArtistCreditIdentity, { account, artist: credit }));
  const first = await screen.findByText(/80 tracks · matched automatically/);
  expect(first.textContent).toContain("Ed Sheeran");
  expect(screen.getByText(/26 tracks · chosen by you/)).toBeTruthy();
  expect(screen.getAllByRole("link", { name: "Open artist" })).toHaveLength(2);
});
