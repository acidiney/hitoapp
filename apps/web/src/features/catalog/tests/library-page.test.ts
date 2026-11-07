import type { Schema } from "../../../shared/schema";
import { createElement, type AnchorHTMLAttributes } from "react";
import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import CatalogPage from "../pages/CatalogPage";
import { libraryDescription } from "../model/libraryCounts";

const player = vi.hoisted(() => ({
  playQueue: vi.fn(),
  subscribe: () => () => {},
  snapshot: () => ({ current: null }),
}));
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => player,
}));
vi.mock("../../../shared/hooks/useMutation", () => ({
  useMutation: () => ({ mutate: vi.fn(), pending: false }),
}));
vi.mock("../../playback/components/TrackRowActions", () => ({
  default: () => null,
}));
vi.mock("../../artwork/components/Artwork", () => ({ default: () => null }));
vi.mock("../../artwork/components/ArtworkStatus", () => ({
  default: () => null,
}));
vi.mock("../../library/hooks/useScanJob", () => ({
  useScanJob: () => ({ scan: vi.fn(), scanning: false }),
}));
vi.mock("@inertiajs/react", () => ({
  router: { get: vi.fn() },
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

afterEach(cleanup);

function track(index: number): Schema<"Recording"> {
  return {
    id: `r${index}`,
    library_id: "personal",
    release_id: "album",
    title: `Track ${index}`,
    inherited_title: `Track ${index}`,
    artist: "Performer",
    inherited_artist: "Performer",
    album: "Album",
    duration_ms: 1000,
    available: true,
    plays_from_services: false,
    liked: false,
  } as Schema<"Recording">;
}

function props(
  overrides: {
    admin?: boolean;
    tracks?: number;
    total?: number;
    files?: number;
    query?: string;
    playlists?: Schema<"Playlist">[];
  } = {},
): Schema<"CatalogPageProps"> {
  const tracks = overrides.tracks ?? 3;
  return {
    errors: {},
    native_playback: false,
    lock: { pin_enabled: false, idle_minutes: 0, locked: false },
    user: {
      id: "account",
      username: "Listener",
      admin: overrides.admin ?? true,
    },
    libraries: [{ id: "personal", name: "Personal", personal: true }],
    playlists: overrides.playlists ?? [],
    query: overrides.query ?? "",
    files: overrides.files ?? tracks,
    recordings: {
      items: Array.from({ length: tracks }, (_, i) => track(i + 1)),
      total: overrides.total ?? tracks,
      offset: 0,
      limit: 100,
    },
  } as Schema<"CatalogPageProps">;
}

const page = (value: Schema<"CatalogPageProps">) =>
  render(createElement(CatalogPage, value));

test("the track grid leads the library with no featured hero and no playlist panel", () => {
  page(props());
  expect(screen.queryByLabelText("Library spotlight")).toBeNull();
  expect(screen.queryByText("Made by you")).toBeNull();
  expect(screen.queryByText("Create your first playlist")).toBeNull();
  expect(screen.getByText("Track 1")).toBeTruthy();
});

test("an administrator can scan from the header when the library has tracks", () => {
  page(props());
  expect(screen.getByRole("button", { name: "Scan music" })).toBeTruthy();
});

test("an empty library gives an administrator one primary button inside the empty card and a clean header", () => {
  page(props({ tracks: 0 }));
  const card = screen.getByRole("heading", {
    name: "Bring your music along",
  }).parentElement!;
  const add = within(card).getByRole("link", { name: "Add a music folder" });
  expect(add.getAttribute("href")).toBe("/settings?section=music");
  expect(add.className).toContain("primary");
  expect(screen.getAllByRole("link", { name: /music folder/i })).toHaveLength(
    1,
  );
  expect(screen.queryByRole("button", { name: /scan music/i })).toBeNull();
  expect(screen.queryByText(/in Settings/)).toBeNull();
});

test("an empty library tells a member that an administrator adds folders and offers no button", () => {
  page(props({ tracks: 0, admin: false }));
  const card = screen.getByRole("heading", {
    name: "Your library is waiting",
  }).parentElement!;
  expect(card.textContent).toContain("administrator");
  expect(within(card).queryByRole("link")).toBeNull();
  expect(within(card).queryByRole("button")).toBeNull();
  expect(screen.queryByRole("button", { name: /scan music/i })).toBeNull();
});

test("a search with no match keeps the scan action and offers no folder button", () => {
  page(props({ tracks: 0, query: "zzz" }));
  expect(screen.getByText("No matching tracks")).toBeTruthy();
  expect(screen.queryByRole("link", { name: "Add a music folder" })).toBeNull();
  expect(screen.getByRole("button", { name: "Scan music" })).toBeTruthy();
});

test("the scan button keeps its name when the label is hidden on a phone", () => {
  page(props());
  const button = screen.getByRole("button", { name: "Scan music" });
  expect(button.querySelector(".button-label")?.textContent).toBe("Scan music");
});

test("the heading counts tracks and says how many files they come from when copies share a recording", () => {
  page(props({ total: 13, files: 27 }));
  expect(
    screen.getByText(
      "13 tracks from 27 files. Copies of one recording count once.",
    ),
  ).toBeTruthy();
});

test("library descriptions", () => {
  expect(libraryDescription({ total: 1, files: 1, query: "" })).toBe("1 track");
  expect(libraryDescription({ total: 5, files: 5, query: "" })).toBe(
    "5 tracks",
  );
  expect(libraryDescription({ total: 5, files: 4, query: "" })).toBe(
    "5 tracks",
  );
  expect(libraryDescription({ total: 1, files: 2, query: "" })).toBe(
    "1 track from 2 files. Copies of one recording count once.",
  );
  expect(libraryDescription({ total: 2, files: 9, query: "ab" })).toBe(
    "2 tracks matching “ab”",
  );
  expect(libraryDescription({ total: 1200, files: 1500, query: "" })).toBe(
    "1,200 tracks from 1,500 files. Copies of one recording count once.",
  );
  expect(libraryDescription({ total: 0, files: 0, query: "" })).toBe(
    "0 tracks",
  );
});
