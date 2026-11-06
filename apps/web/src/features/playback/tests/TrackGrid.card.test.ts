import type { Schema } from "../../../shared/schema";
import { createElement, type AnchorHTMLAttributes } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import TrackGrid from "../components/TrackGrid";
import type { TrackRow } from "../components/TrackTable";

const player = vi.hoisted(() => ({
  playQueue: vi.fn(async () => {}),
  addToQueue: vi.fn(async () => {}),
  playNext: vi.fn(async () => {}),
  subscribe: () => () => {},
  snapshot: () => ({ current: null }),
}));
const router = vi.hoisted(() => ({ visit: vi.fn(), reload: vi.fn() }));
vi.mock("../components/PlaybackProvider", () => ({ usePlayer: () => player }));
vi.mock("../../artwork/components/Artwork", () => ({ default: () => null }));
vi.mock("@inertiajs/react", () => ({
  router,
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

const playlists: Schema<"Playlist">[] = [
  { id: "late", name: "Late night", kind: "local" } as Schema<"Playlist">,
];

const tracks: TrackRow[] = [
  {
    id: "rec-1",
    row_id: "row-1",
    title: "Salt Flats",
    artist: "Cartographers",
    artist_id: "artist-1",
    album: "Atlas",
    release_id: "release-1",
    duration_ms: 185_000,
    available: true,
    liked: true,
    inherited_title: "Salt Flats",
    inherited_artist: "Cartographers",
    plays_from_services: false,
  },
  {
    id: "rec-2",
    row_id: "row-2",
    title: "Meridian",
    artist: "Cartographers",
    artist_id: null,
    album: null,
    duration_ms: 90_000,
    available: true,
    liked: false,
    inherited_title: "Meridian",
    inherited_artist: "Cartographers",
    plays_from_services: false,
  },
];

test("a card plays from its position, links its names and carries the row actions", () => {
  const mutate = vi.fn(async () => ({ ok: true as const }));
  render(createElement(TrackGrid, { tracks, playlists, mutate }));
  const cards = screen.getAllByRole("article");
  const second = within(cards[1]);
  fireEvent.click(second.getByRole("button", { name: "Play Meridian" }));
  expect(player.playQueue).toHaveBeenCalledWith(
    expect.arrayContaining([expect.objectContaining({ id: "rec-2" })]),
    1,
    undefined,
  );
  const first = within(cards[0]);
  expect(
    first.getByRole("link", { name: "Salt Flats" }).getAttribute("href"),
  ).toBe("/recordings/rec-1");
  expect(
    first.getByRole("link", { name: "Cartographers" }).getAttribute("href"),
  ).toBe("/artists/artist-1");
  expect(first.getByRole("link", { name: "Atlas" }).getAttribute("href")).toBe(
    "/albums/release-1",
  );
  expect(second.queryByRole("link", { name: "Cartographers" })).toBeNull();
  expect(
    first
      .getByRole("button", { name: "Unlike Salt Flats" })
      .getAttribute("aria-pressed"),
  ).toBe("true");
  fireEvent.click(
    first.getByRole("button", { name: "Actions for Salt Flats" }),
  );
  expect(
    screen.getAllByRole("menuitem").map((item) => item.textContent),
  ).toEqual([
    "Track details",
    "Play next",
    "Add to queue",
    "Add to playlist",
    "Fix source",
  ]);
  expect(screen.queryByRole("combobox")).toBeNull();
});
