import type { Schema } from "../../../shared/schema";
import { createElement, type AnchorHTMLAttributes } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ArtistIdentityGroupPage from "../pages/ArtistIdentityGroupPage";
import { artistIdentityBrowsing } from "../fixtures/artistIdentityBrowsing";
import { artistGroupHttp } from "../fixtures/artistGroupDiscography";
import {
  artistAssociation,
  reviewedArtistProfile,
} from "../fixtures/artistReview";

const player = vi.hoisted(() => ({
  playQueue: vi.fn(),
  subscribe: () => () => {},
  snapshot: () => ({ current: null }),
  subscribePosition: vi.fn(),
}));
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => player,
}));
vi.mock("../../../shared/hooks/useMutation", () => ({
  useMutation: () => ({ mutate: vi.fn(), pending: false }),
}));
vi.mock("@inertiajs/react", () => ({
  router: { get: vi.fn() },
  usePage: () => ({ url: window.location.pathname + window.location.search }),
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

afterEach(() => {
  cleanup();
  window.history.replaceState(null, "", "/");
});

function release(
  id: string,
  title: string,
  kind: Schema<"CatalogReleaseGroup">["kind"],
  date: string | null,
): Schema<"CatalogReleaseGroup"> {
  return {
    id,
    title,
    artist_credit: [{ credited_name: "Adele", join_phrase: "" }],
    kind,
    secondary_types: [],
    first_release_date: date,
    revision: null,
  };
}

async function open() {
  const http = artistGroupHttp();
  http.reply("discography", {
    status: "available",
    association_id: artistAssociation.id,
    continuation: null,
    page: {
      artist_id: reviewedArtistProfile.profile.artist.id,
      snapshot: null,
      total_count: null,
      next_cursor: null,
      items: [
        release("rg:owned", "Hometown Glory", "single", "2008-04-20"),
        release("rg:missing", "Hello", "album", null),
      ],
    },
  });
  http.replyLibrary({
    association_id: artistAssociation.id,
    groups: [
      {
        release_group_id: "rg:owned",
        albums: [
          {
            release_id: "album-owned",
            edition_id: "edition:1",
            track_count: 4,
            playable_count: 4,
          },
        ],
      },
      { release_group_id: "rg:missing", albums: [] },
    ],
  });
  const { detail } = artistIdentityBrowsing();
  render(createElement(ArtistIdentityGroupPage, detail));
  const region = await screen.findByRole("region", { name: "Discography" });
  await within(region).findByRole("img", { name: "In your library" });
  return region;
}

test("a release card shows the type and year only, with open and play as separate controls", async () => {
  const region = await open();
  const card = within(
    within(region)
      .getByRole("heading", { name: "Hometown Glory" })
      .closest("li") as HTMLElement,
  );
  expect(card.getByText("Single · 2008")).toBeTruthy();
  expect(card.queryByText(/2008-04-20/)).toBeNull();
  expect(
    card.getByRole("button", { name: "Open Hometown Glory" }),
  ).toBeTruthy();
  expect(
    card.getByRole("button", { name: "Play Hometown Glory" }),
  ).toBeTruthy();
  expect(card.queryByRole("button", { name: "Open album" })).toBeNull();
  expect(card.queryByRole("button", { name: "Play album" })).toBeNull();
  expect(card.queryByText("Not in your library")).toBeNull();
  expect(card.getByRole("img", { name: "In your library" })).toBeTruthy();
});

test("a release that is not in the library shows no badge and no mark", async () => {
  const region = await open();
  const card = within(
    within(region)
      .getByRole("heading", { name: "Hello" })
      .closest("li") as HTMLElement,
  );
  expect(card.getByText("Album")).toBeTruthy();
  expect(card.queryByText("Not in your library")).toBeNull();
  expect(card.queryByRole("img", { name: "In your library" })).toBeNull();
});

test("the card opens the album in place and its library links appear there", async () => {
  const region = await open();
  const card = within(
    within(region)
      .getByRole("heading", { name: "Hometown Glory" })
      .closest("li") as HTMLElement,
  );
  expect(card.queryByRole("link", { name: "Open in your library" })).toBeNull();
  fireEvent.click(card.getByRole("button", { name: "Open Hometown Glory" }));
  expect(
    await card.findByRole("link", { name: "Open in your library" }),
  ).toBeTruthy();
});
