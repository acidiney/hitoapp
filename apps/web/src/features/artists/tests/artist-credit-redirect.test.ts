import { createElement, type AnchorHTMLAttributes } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ArtistCreditIdentity from "../components/ArtistCreditIdentity";
import type { Schema } from "../../../shared/schema";

const visit = vi.fn();
vi.mock("@inertiajs/react", () => ({
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
  router: { reload: vi.fn(), visit: (...args: unknown[]) => visit(...args) },
}));

const account = "10000000-0000-4000-8000-000000000001";
const credit = { id: "credit-one", name: "C4 Pedro" };
const chosenGroup: Schema<"ArtistIdentityGroup"> = {
  id: "group-one",
  provider: { kind: "builtin_musicbrainz" },
  provider_artist_id: "11111111-1111-4111-8111-111111111111",
  name: {
    value: "C4 Pedro",
    recording_id: "r1",
    artist_id: "a1",
    association_id: "s1",
  },
  library_count: 1,
  recording_count: 3,
  release_count: 1,
};

function suggestions(undecided: number, chosen: boolean) {
  return {
    artist_id: credit.id,
    chosen: chosen
      ? [
          {
            group: chosenGroup,
            tracks: 3,
            automatic_tracks: 3,
            evidence: [],
          },
        ]
      : [],
    undecided_tracks: undecided,
    dismissed: 0,
    dismissed_items: [],
    items: [],
  };
}

function serve(value: unknown) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string) =>
      path.endsWith("/match-suggestions")
        ? Response.json(value)
        : Response.json({
            enabled: true,
            state: "idle",
            matched_so_far: 0,
            last_finished_at: null,
            last_matched: 0,
          }),
    ),
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
  visit.mockClear();
});

test("a fully identified credit replaces itself with the chosen artist page", async () => {
  serve(suggestions(0, true));
  render(createElement(ArtistCreditIdentity, { account, artist: credit }));
  await waitFor(() =>
    expect(visit).toHaveBeenCalledWith("/artist-groups/group-one", {
      replace: true,
    }),
  );
});

test("a partly identified credit stays and links to the chosen artist", async () => {
  serve(suggestions(2, true));
  render(createElement(ArtistCreditIdentity, { account, artist: credit }));
  const link = await screen.findByRole("link", { name: "Open artist" });
  expect(link.getAttribute("href")).toBe("/artist-groups/group-one");
  expect(screen.getByText(/Also listed as/)).toBeTruthy();
  expect(visit).not.toHaveBeenCalled();
});

test("an unidentified credit keeps its page with a choose-the-artist row", async () => {
  serve(suggestions(3, false));
  render(createElement(ArtistCreditIdentity, { account, artist: credit }));
  await screen.findByText("Not identified yet");
  expect(
    screen.getByRole("button", { name: "Choose the artist" }),
  ).toBeTruthy();
  expect(visit).not.toHaveBeenCalled();
});
