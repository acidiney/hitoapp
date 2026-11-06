import type { Schema } from "../../../shared/schema";
import { createElement, type AnchorHTMLAttributes } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import TrackTable, {
  type TrackRow,
  type TrackTableRow,
} from "../components/TrackTable";

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
  { id: "feed", name: "Followed", kind: "followed" } as Schema<"Playlist">,
];

function track(over: Partial<TrackRow> = {}): TrackRow {
  return {
    id: "rec-1",
    row_id: "row-1",
    title: "Salt Flats",
    artist: "Cartographers",
    artist_id: "artist-1",
    album: "Atlas",
    release_id: "release-1",
    duration_ms: 185_000,
    available: true,
    liked: false,
    inherited_title: "Salt Flats",
    inherited_artist: "Cartographers",
    plays_from_services: false,
    ...over,
  };
}

function show(
  rows: TrackTableRow[],
  over: Partial<Parameters<typeof TrackTable>[0]> = {},
) {
  const mutate = vi.fn(async () => ({ ok: true as const }));
  render(createElement(TrackTable, { rows, playlists, mutate, ...over }));
  return { mutate, row: within(screen.getAllByRole("row")[1]) };
}

const recording = (over: Partial<TrackRow> = {}): TrackTableRow => ({
  kind: "recording",
  track: track(over),
});

test("a row links the title, artist and album and shows the duration", () => {
  const { row } = show([recording()]);
  expect(
    row.getByRole("link", { name: "Salt Flats" }).getAttribute("href"),
  ).toBe("/recordings/rec-1");
  const artistLinks = row.getAllByRole("link", { name: "Cartographers" });
  const albumLinks = row.getAllByRole("link", { name: "Atlas" });
  expect(artistLinks.map((link) => link.getAttribute("href"))).toEqual([
    "/artists/artist-1",
    "/artists/artist-1",
  ]);
  expect(albumLinks.map((link) => link.getAttribute("href"))).toEqual([
    "/albums/release-1",
    "/albums/release-1",
  ]);
  expect(row.getByText("3:05")).toBeTruthy();
  expect(row.queryByRole("combobox")).toBeNull();
});

test("an artist without a page stays plain text", () => {
  const { row } = show([recording({ artist_id: null })]);
  expect(row.queryByRole("link", { name: "Cartographers" })).toBeNull();
  expect(row.getAllByText("Cartographers").length).toBeGreaterThan(0);
});

test("the play button is the row's play path and queues from that row", () => {
  const { row } = show([
    recording(),
    recording({ id: "rec-2", row_id: "row-2" }),
  ]);
  fireEvent.click(row.getByRole("button", { name: "Play Salt Flats" }));
  expect(player.playQueue).toHaveBeenCalledWith(
    expect.arrayContaining([expect.objectContaining({ id: "rec-1" })]),
    0,
    undefined,
  );
});

test("the heart toggles the like through the mutation and reloads", async () => {
  const { row, mutate } = show([recording()]);
  fireEvent.click(row.getByRole("button", { name: "Like Salt Flats" }));
  await vi.waitFor(() => expect(router.reload).toHaveBeenCalled());
  expect(mutate).toHaveBeenCalledWith({
    kind: "like_set",
    recording_id: "rec-1",
    liked: true,
  });
});

test("the row menu opens from the keyboard and lists every action", () => {
  const { row } = show([recording()]);
  const trigger = row.getByRole("button", { name: "Actions for Salt Flats" });
  fireEvent.click(trigger);
  const menu = screen.getByRole("menu", { name: "Actions for Salt Flats" });
  expect(
    within(menu)
      .getAllByRole("menuitem")
      .map((item) => item.textContent),
  ).toEqual([
    "Track details",
    "Play next",
    "Add to queue",
    "Add to playlist",
    "Fix source",
  ]);
  expect(document.activeElement).toBe(
    within(menu).getByRole("menuitem", { name: "Track details" }),
  );
  fireEvent.keyDown(document.activeElement!, { key: "ArrowDown" });
  expect(document.activeElement).toBe(
    within(menu).getByRole("menuitem", { name: "Play next" }),
  );
  fireEvent.keyDown(document.activeElement!, { key: "Escape" });
  expect(screen.queryByRole("menu")).toBeNull();
  expect(document.activeElement).toBe(trigger);
});

test("play next and add to queue hand the track to the player", () => {
  const { row } = show([recording()]);
  fireEvent.click(row.getByRole("button", { name: "Actions for Salt Flats" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Play next" }));
  expect(player.playNext).toHaveBeenCalledWith(
    expect.objectContaining({ id: "rec-1" }),
  );
  fireEvent.click(row.getByRole("button", { name: "Actions for Salt Flats" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Add to queue" }));
  expect(player.addToQueue).toHaveBeenCalledWith(
    expect.objectContaining({ id: "rec-1" }),
  );
});

test("add to playlist offers only editable playlists in a submenu", async () => {
  const { row, mutate } = show([recording()]);
  fireEvent.click(row.getByRole("button", { name: "Actions for Salt Flats" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Add to playlist" }));
  const submenu = screen.getByRole("menu", { name: "Add to playlist" });
  expect(
    within(submenu).queryByRole("menuitem", { name: "Followed" }),
  ).toBeNull();
  fireEvent.click(
    within(submenu).getByRole("menuitem", { name: "Late night" }),
  );
  await vi.waitFor(() => expect(router.reload).toHaveBeenCalled());
  expect(mutate).toHaveBeenCalledWith(
    expect.objectContaining({
      kind: "entry_insert",
      playlist_id: "late",
      recording_id: "rec-1",
    }),
  );
  expect(screen.queryByRole("menu")).toBeNull();
});

test("track details and the source item open the track page", () => {
  const { row } = show([recording({ available: false })]);
  fireEvent.click(row.getByRole("button", { name: "Actions for Salt Flats" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Track details" }));
  expect(router.visit).toHaveBeenLastCalledWith("/recordings/rec-1");
  fireEvent.click(row.getByRole("button", { name: "Actions for Salt Flats" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Find audio" }));
  expect(router.visit).toHaveBeenLastCalledWith("/recordings/rec-1#find-audio");
});

test("columns that repeat the page context are left out", () => {
  show([recording()], { without: ["artist", "album"] });
  const headers = screen.getAllByRole("columnheader").map((h) => h.textContent);
  expect(headers).not.toContain("Artist");
  expect(headers).not.toContain("Album");
  expect(screen.queryByRole("link", { name: "Atlas" })).toBeNull();
});

test("the album column disappears when no row knows its album", () => {
  show([recording({ album: null, release_id: null })]);
  const headers = screen.getAllByRole("columnheader").map((h) => h.textContent);
  expect(headers).not.toContain("Album");
  expect(headers).toContain("Artist");
});

test("a local playlist adds remove to the menu", async () => {
  const playlist = playlists[0];
  const { row, mutate } = show([recording({ entry_id: "entry-1" })], {
    playlist,
  });
  fireEvent.click(row.getByRole("button", { name: "Actions for Salt Flats" }));
  fireEvent.click(
    screen.getByRole("menuitem", { name: "Remove from playlist" }),
  );
  await vi.waitFor(() =>
    expect(mutate).toHaveBeenCalledWith({
      kind: "entry_remove",
      playlist_id: "late",
      entry_id: "entry-1",
    }),
  );
});
