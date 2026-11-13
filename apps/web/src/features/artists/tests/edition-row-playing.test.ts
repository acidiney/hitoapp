import { createElement } from "react";
import { render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import CatalogTrackList from "../components/CatalogTrackList";
import AlbumEditionTrackList from "../components/AlbumEditionTrackList";
import type { CatalogTrack } from "../hooks/useCatalogTracks";
import { foundRecording } from "../fixtures/catalogAlbumPlay";
import {
  albumId,
  artistReviewAccount,
  firstLibraryRecording,
  mergedTrackList,
} from "../../catalog/fixtures/albumEditions";

const now = vi.hoisted(() => ({
  current: null as { id: string } | null,
  playing: true,
  positionReads: 0,
}));
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => ({
    subscribe: () => () => {},
    snapshot: () => ({ current: now.current, playing: now.playing }),
    subscribePosition: () => {
      now.positionReads++;
      return () => {};
    },
    playQueue: vi.fn(),
    prepareSelection: vi.fn(),
    queueSnapshot: () => ({ currentIndex: null, total: 0 }),
  }),
}));
vi.mock("@inertiajs/react", () => ({
  router: { get: vi.fn(), visit: vi.fn() },
}));

const track = (index: number, owned: boolean): CatalogTrack => ({
  id: `occ-${index}`,
  number: String(index + 1),
  title: `Song ${index + 1}`,
  credit: "Performer",
  durationMs: 1000,
  status: "idle",
  recording: owned ? foundRecording(index) : null,
});

function catalogList() {
  render(
    createElement(CatalogTrackList, {
      tracks: [track(0, true), track(1, true), track(2, false)],
      controls: { play: vi.fn(), find: vi.fn(), open: vi.fn(), like: vi.fn() },
      filter: "all",
      playlists: [],
      mutate: vi.fn(async () => ({ ok: true as const })),
    }),
  );
  return within(screen.getByRole("list", { name: "Tracks" })).getAllByRole(
    "listitem",
  );
}

test("the playing catalog row shows the bars, is announced and is the only current row", () => {
  now.current = { id: foundRecording(1).id };
  now.playing = true;
  const rows = catalogList();
  const current = rows.filter(
    (row) => row.getAttribute("aria-current") === "true",
  );
  expect(current).toHaveLength(1);
  expect(current[0].textContent).toContain("Song 2");
  expect(current[0].querySelectorAll(".playing-bars")).toHaveLength(1);
  expect(current[0].textContent).toContain("Now playing");
  expect(rows[0].querySelector(".playing-bars")).toBeNull();
  expect(rows[2].querySelector(".playing-bars")).toBeNull();
  expect(now.positionReads).toBe(0);
});

test("a paused current row keeps still bars and says paused", () => {
  now.current = { id: foundRecording(0).id };
  now.playing = false;
  const [first] = catalogList();
  expect(first.querySelector(".playing-bars")?.getAttribute("data-state")).toBe(
    "paused",
  );
  expect(first.textContent).toContain("Paused");
});

test("the playing owned row of a library album's edition list shows the bars", () => {
  now.current = { id: firstLibraryRecording.id };
  now.playing = true;
  render(
    createElement(AlbumEditionTrackList, {
      account: artistReviewAccount,
      source: { release: albumId, recording: firstLibraryRecording.id },
      comparison: mergedTrackList,
      pending: false,
      error: "",
      retry: async () => {},
    }),
  );
  const region = screen.getByRole("region", {
    name: "Album edition track list",
  });
  const current = within(region)
    .getAllByRole("listitem")
    .filter((row) => row.getAttribute("aria-current") === "true");
  expect(current.length).toBeGreaterThan(0);
  for (const row of current)
    expect(row.querySelector(".playing-bars")).not.toBeNull();
  expect(now.positionReads).toBe(0);
});
