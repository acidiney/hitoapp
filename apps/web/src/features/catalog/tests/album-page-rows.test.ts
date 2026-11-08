import type { Schema } from "../../../shared/schema";
import { createElement, type AnchorHTMLAttributes } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import AlbumPage from "../pages/AlbumPage";

const artwork = vi.hoisted(() => ({
  state: { imageFailed: false, phase: "ready", retry: vi.fn() },
}));
vi.mock("../../artwork/components/ArtworkProvider", () => ({
  useArtwork: () => artwork.state,
}));
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => ({
    playQueue: vi.fn(),
    subscribe: () => () => {},
    snapshot: () => ({ current: null }),
  }),
}));
vi.mock("../../../shared/hooks/useMutation", () => ({
  useMutation: () => ({ mutate: vi.fn(), pending: false }),
}));
vi.mock("@inertiajs/react", () => ({
  router: { get: vi.fn() },
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

const track = {
  id: "recording",
  library_id: "library",
  release_id: "album",
  title: "Opening",
  inherited_title: "Opening",
  artist: "Performer",
  inherited_artist: "Performer",
  album: "Two discs",
  duration_ms: 1000,
  available: true,
  plays_from_services: false,
  liked: false,
};

function page(): Schema<"AlbumPageProps"> {
  return {
    errors: {},
    native_playback: false,
    lock: { pin_enabled: false, idle_minutes: 0, locked: false },
    user: { id: "account", username: "Listener", admin: false },
    libraries: [],
    playlists: [],
    detail: {
      release: {
        id: "album",
        library_id: "library",
        title: "Two discs",
        artist: "Performer",
        artist_id: "artist",
        inferred_artist: null,
        track_count: 3,
        duration_ms: 3000,
        artwork_recording_id: "recording",
      },
      tracks: {
        total: 3,
        offset: 0,
        limit: 60,
        items: [
          { ...track, entry_id: "one", disc_number: 1, track_number: 1 },
          {
            ...track,
            title: "Second",
            entry_id: "two",
            disc_number: 1,
            track_number: 2,
          },
          {
            ...track,
            title: "Reprise",
            entry_id: "three",
            disc_number: 2,
            track_number: 1,
          },
        ],
      },
      catalog_anchor: null,
      saved_edition: null,
    },
  };
}

test("album rows show disc and track numbers and no repeated artist or album columns", () => {
  render(createElement(AlbumPage, page()));
  const table = screen.getByRole("table");
  expect(
    within(table).queryByRole("columnheader", { name: "Artist" }),
  ).toBeNull();
  expect(
    within(table).queryByRole("columnheader", { name: "Album" }),
  ).toBeNull();
  const numbers = within(table)
    .getAllByRole("row")
    .slice(1)
    .map((row) => within(row).getAllByRole("cell")[0].textContent);
  expect(numbers.map((text) => text?.replace(/\D*$/, ""))).toEqual([
    "1",
    "2",
    "2.1",
  ]);
});

test("a working album shows no retry control", () => {
  artwork.state = { imageFailed: false, phase: "ready", retry: vi.fn() };
  render(createElement(AlbumPage, page()));
  expect(screen.queryByText("Retry artwork")).toBeNull();
  expect(screen.queryByRole("button", { name: "More options" })).toBeNull();
});

test("an album without a cover source keeps Retry artwork inside the header menu", () => {
  const retry = vi.fn();
  artwork.state = { imageFailed: true, phase: "unavailable", retry };
  render(createElement(AlbumPage, page()));
  expect(screen.queryByText("Retry artwork")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Retry artwork" }));
  expect(retry).toHaveBeenCalledTimes(1);
});

test("a failed album cover explains the failure and offers the retry in the page", () => {
  artwork.state = { imageFailed: true, phase: "failed", retry: vi.fn() };
  render(createElement(AlbumPage, page()));
  expect(screen.getByText("Artwork could not be retrieved.")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Retry artwork" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "More options" })).toBeNull();
});
