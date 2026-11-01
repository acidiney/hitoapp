import type { Schema } from "../../../shared/schema";
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

function release(id: string): Schema<"CatalogReleaseGroup"> {
  return {
    id,
    title: `Title of ${id}`,
    artist_credit: null,
    kind: "album",
    secondary_types: [],
    first_release_date: null,
    revision: null,
  };
}

const album = (releaseId: string): Schema<"LibraryAlbum"> => ({
  release_id: releaseId,
  edition_id: "edition:1",
  track_count: 12,
  playable_count: 12,
});

async function open(albumsByGroup: Record<string, string[]>) {
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
      items: Object.keys(albumsByGroup).map(release),
    },
  });
  http.replyLibrary({
    association_id: artistAssociation.id,
    groups: Object.entries(albumsByGroup).map(([id, albums]) => ({
      release_group_id: id,
      albums: albums.map(album),
    })),
  });
  const { detail } = artistIdentityBrowsing();
  render(createElement(ArtistIdentityGroupPage, detail));
  const region = await screen.findByRole("region", { name: "Discography" });
  for (const id of Object.keys(albumsByGroup))
    fireEvent.click(
      await within(region).findByRole("button", {
        name: `Open Title of ${id}`,
      }),
    );
  await within(region).findAllByText(/Compare|Not linked/);
  return { http, region };
}

const compare = { name: "Compare with the saved edition" };

test("a linked album is compared only when asked and shows its level with the track count", async () => {
  const { http, region } = await open({ "rg:one": ["album/é"] });
  expect(http.levelRequests).toEqual([]);
  http.replyLevel({
    status: "compared",
    level: "partly",
    matched_tracks: 7,
    edition_tracks: 12,
  });
  fireEvent.click(within(region).getByRole("button", compare));
  await within(region).findByText("Partly in library · 7 of 12 tracks");
  expect(http.levelRequests).toEqual([
    { release: "album/é", body: { release_group_id: "rg:one" } },
  ]);
  expect(
    within(region)
      .getByText("Partly in library · 7 of 12 tracks")
      .closest(".availability")
      ?.getAttribute("data-state"),
  ).toBe("available");
  expect(within(region).queryByRole("button", compare)).toBeNull();
  expect(player.playQueue).not.toHaveBeenCalled();
});

test("each level keeps its own words, state and exact counts", async () => {
  const { http, region } = await open({ "rg:a": ["one"], "rg:b": ["two"] });
  http.replyLevel({
    status: "compared",
    level: "in_library",
    matched_tracks: 12,
    edition_tracks: 12,
  });
  http.replyLevel({
    status: "compared",
    level: "linked",
    matched_tracks: 0,
    edition_tracks: 12,
  });
  const buttons = within(region).getAllByRole("button", compare);
  fireEvent.click(buttons[0]);
  const full = await within(region).findByText("In library · 12 of 12 tracks");
  expect(full.closest(".availability")?.getAttribute("data-state")).toBe(
    "library",
  );
  fireEvent.click(within(region).getByRole("button", compare));
  const none = await within(region).findByText("Linked · 0 of 12 tracks");
  expect(none.closest(".availability")?.getAttribute("data-state")).toBe(
    "catalog",
  );
  expect(http.levelRequests.map((request) => request.release)).toEqual([
    "one",
    "two",
  ]);
});

test("a release group without a library link offers no comparison", async () => {
  const { http, region } = await open({ "rg:none": [] });
  expect(within(region).queryByRole("button", compare)).toBeNull();
  expect(http.levelRequests).toEqual([]);
});

test("a missing remembered edition asks for a choice without a retry loop", async () => {
  const { http, region } = await open({ "rg:one": ["one"] });
  http.replyLevel({ status: "needs_choice" });
  fireEvent.click(within(region).getByRole("button", compare));
  await within(region).findByText(
    "Choose an edition on the album page to compare it.",
  );
  expect(http.levelRequests).toHaveLength(1);
});

test("a provider outage is retryable and keeps the link to the album", async () => {
  const { http, region } = await open({ "rg:one": ["one"] });
  http.replyLevel({ status: "unavailable" });
  fireEvent.click(within(region).getByRole("button", compare));
  expect(
    await within(region).findByText(
      "The edition could not be read. Try again.",
    ),
  ).toBeTruthy();
  expect(
    within(region).getByRole("link", { name: "Open in your library" }),
  ).toBeTruthy();
  http.replyLevel({
    status: "compared",
    level: "partly",
    matched_tracks: 1,
    edition_tracks: 2,
  });
  fireEvent.click(within(region).getByRole("button", { name: "Try again" }));
  await within(region).findByText("Partly in library · 1 of 2 tracks");
  expect(http.levelRequests).toHaveLength(2);
});

test("an error response is reported and can be retried", async () => {
  const { http, region } = await open({ "rg:one": ["one"] });
  http.replyLevel({ code: "internal", message: "broken" }, 500);
  fireEvent.click(within(region).getByRole("button", compare));
  await within(region).findByRole("alert");
  await waitFor(() =>
    expect(
      within(region).getByRole("button", { name: "Try again" }),
    ).toBeTruthy(),
  );
});
