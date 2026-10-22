import { createElement, type AnchorHTMLAttributes } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import AlbumPage from "./pages/AlbumPage";
import ArtistPage from "./pages/ArtistPage";
import Pagination from "./shared/ui/Pagination";
import TrackToolbar from "./shared/ui/TrackToolbar";
import AlbumGrid from "./shared/ui/AlbumGrid";
import type { AlbumPageProps, Release } from "./shared/types";

const player = vi.hoisted(() => ({
  playQueue: vi.fn(),
  subscribe: () => () => {},
  snapshot: () => ({ current: null }),
}));
vi.mock("./player/PlaybackProvider", () => ({ usePlayer: () => player }));
vi.mock("./shared/hooks/useMutation", () => ({
  useMutation: () => ({ mutate: vi.fn(), pending: false }),
}));
const navigation = vi.hoisted(() => ({ get: vi.fn() }));
vi.mock("@inertiajs/react", () => ({
  router: navigation,
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));
test("album playback keeps repeated recording occurrences and their tagged order", () => {
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
    liked: false,
  };
  render(
    createElement(AlbumPage, {
      errors: {},
      native_playback: false,
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
          track_count: 2,
          duration_ms: 2000,
          artwork_recording_id: "recording",
        },
        tracks: {
          total: 2,
          offset: 0,
          limit: 60,
          items: [
            { ...track, entry_id: "one", disc_number: 1, track_number: 1 },
            {
              ...track,
              title: "Reprise",
              entry_id: "two",
              disc_number: 2,
              track_number: 1,
            },
          ],
        },
      },
    }),
  );
  expect(screen.getByRole("heading", { name: "Two discs" })).toBeTruthy();
  expect(
    screen.getByRole("link", { name: "Performer" }).getAttribute("href"),
  ).toBe("/artists/artist");
  expect(
    [...document.querySelectorAll("img")].map((image) =>
      image.getAttribute("src"),
    ),
  ).toEqual([
    "/api/v1/releases/album/artwork",
    "/api/v1/releases/album/artwork",
    "/api/v1/releases/album/artwork",
  ]);
  fireEvent.click(screen.getByRole("button", { name: "Play album" }));
  const queued = player.playQueue.mock.calls[0][0];
  expect(queued.map((item: { title: string }) => item.title)).toEqual([
    "Opening",
    "Reprise",
  ]);
  expect(queued.map((item: { row_id: string }) => item.row_id)).toEqual([
    "one",
    "two",
  ]);
});

test("an album with unknown credit keeps its card and detail links without inventing a performer", () => {
  const release: Release = {
    id: "unknown-credit-album",
    library_id: "personal",
    title: "Owned source-free album",
    artist: null,
    artist_id: null,
    track_count: 2,
    duration_ms: 2000,
    artwork_recording_id: "recording-one",
  };
  const tracks: AlbumPageProps["detail"]["tracks"]["items"] = [
    {
      id: "recording-one",
      entry_id: "entry-one",
      library_id: "personal",
      release_id: release.id,
      title: "Opening",
      inherited_title: "Opening",
      artist: "Required first performer",
      inherited_artist: "Required first performer",
      album: release.title,
      duration_ms: 1000,
      available: false,
      liked: false,
      disc_number: 1,
      track_number: 1,
    },
    {
      id: "recording-two",
      entry_id: "entry-two",
      library_id: "personal",
      release_id: release.id,
      title: "Reprise",
      inherited_title: "Reprise",
      artist: "Required second performer",
      inherited_artist: "Required second performer",
      album: release.title,
      duration_ms: 1000,
      available: false,
      liked: false,
      disc_number: 2,
      track_number: 1,
    },
  ];
  const card = render(createElement(AlbumGrid, { releases: [release] }));
  expect(
    screen.getByText("Album artist not provided", { exact: true }),
  ).toBeTruthy();
  expect(
    screen
      .getByRole("link", { name: `Open ${release.title}` })
      .getAttribute("href"),
  ).toBe(`/albums/${release.id}`);
  expect(
    screen.getByRole("link", { name: release.title }).getAttribute("href"),
  ).toBe(`/albums/${release.id}`);
  expect(document.querySelectorAll('a[href^="/artists/"]')).toHaveLength(0);
  expect(
    document.querySelector(
      '[aria-label*="by null"], [aria-label*="by undefined"]',
    ),
  ).toBeNull();
  card.unmount();
  render(
    createElement(AlbumPage, {
      errors: {},
      native_playback: false,
      user: { id: "account", username: "Listener", admin: false },
      libraries: [],
      playlists: [],
      detail: {
        release,
        tracks: { total: 2, offset: 0, limit: 60, items: tracks },
      },
    }),
  );
  expect(screen.getByRole("heading", { name: release.title })).toBeTruthy();
  expect(
    screen.getByText("Album artist not provided", { exact: true }),
  ).toBeTruthy();
  expect(document.querySelectorAll('a[href^="/artists/"]')).toHaveLength(0);
  expect(
    document.querySelectorAll(
      'a[href="/artists/null"], a[href="/artists/undefined"]',
    ),
  ).toHaveLength(0);
  expect(
    screen
      .getByRole("link", { name: "Edit album details" })
      .getAttribute("href"),
  ).toBe(`/albums/${release.id}/details`);
  expect(
    [...document.querySelectorAll("tbody .track-title")].map(
      (node) => node.textContent,
    ),
  ).toEqual(["Opening", "Reprise"]);
  expect(
    [...document.querySelectorAll("tbody .artist-column")].map(
      (node) => node.textContent,
    ),
  ).toEqual(["Required first performer", "Required second performer"]);
  expect(
    [...document.querySelectorAll("tbody .row-play")].map(
      (node) => node.textContent,
    ),
  ).toEqual(["1", "2.1"]);
  expect(
    screen.getAllByText("Unavailable on this device", { exact: true }),
  ).toHaveLength(2);
  expect(player.playQueue).not.toHaveBeenCalled();
  expect(release.artist).toBeNull();
  expect(release.artist_id).toBeNull();
});

test("browse pagination retains the active library and page size", () => {
  window.history.replaceState(
    null,
    "",
    "/albums?library=selected&limit=1&q=Quiet",
  );
  render(
    createElement(Pagination, {
      page: { total: 3, limit: 1, offset: 0 },
      query: "Quiet",
      path: "/albums",
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  expect(navigation.get).toHaveBeenCalledWith("/albums", {
    library: "selected",
    limit: "1",
    q: "Quiet",
    offset: 1,
  });
});

test("albumless artist tracks play only their page and keep album pagination separate", () => {
  window.history.replaceState(null, "", "/artists/credit?offset=4&limit=1");
  const track = {
    id: "provider-recording",
    library_id: "personal",
    release_id: null,
    title: "Saved recording",
    inherited_title: "Saved recording",
    artist: "Provider performer",
    inherited_artist: "Provider performer",
    album: null,
    duration_ms: 1000,
    available: true,
    liked: false,
  };
  render(
    createElement(ArtistPage, {
      errors: {},
      native_playback: false,
      user: { id: "account", username: "Listener", admin: false },
      libraries: [],
      playlists: [],
      detail: {
        artist: {
          id: "credit",
          library_id: "personal",
          name: "Provider performer",
          release_count: 0,
          recording_count: 2,
          artwork_recording_id: track.id,
        },
        releases: { total: 0, offset: 4, limit: 1, items: [] },
        recordings: { total: 2, offset: 0, limit: 1, items: [track] },
      },
    }),
  );
  expect(
    screen.getByRole("heading", { name: "Provider performer" }),
  ).toBeTruthy();
  expect(
    screen.queryByRole("heading", { name: "Albums & appearances" }),
  ).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Play this page" }));
  expect(
    player.playQueue.mock.calls[0][0].map((item: { id: string }) => item.id),
  ).toEqual([track.id]);
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  expect(navigation.get).toHaveBeenCalledWith("/artists/credit", {
    offset: "4",
    limit: "1",
    recording_offset: 1,
  });
});

test("known recording collections allow playback resolution while empty collections cannot start", () => {
  const { rerender } = render(
    createElement(TrackToolbar, {
      tracks: [
        {
          id: "saved",
          title: "Unavailable recording",
          artist: "Artist",
          duration_ms: 0,
          available: false,
        },
      ],
      description: "Recording credits in your library",
    }),
  );
  const button = screen.getByRole("button", {
    name: "Play tracks",
  }) as HTMLButtonElement;
  expect(button.disabled).toBe(false);
  fireEvent.click(button);
  expect(player.playQueue).toHaveBeenCalledWith([
    expect.objectContaining({ id: "saved", available: false }),
  ]);
  rerender(
    createElement(TrackToolbar, {
      tracks: [],
      description: "Recording credits in your library",
    }),
  );
  expect(
    (screen.getByRole("button", { name: "Play tracks" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
});

test("an emptied track page keeps a way back without moving the album page", () => {
  window.history.replaceState(
    null,
    "",
    "/artists/credit?offset=4&recording_offset=60",
  );
  render(
    createElement(Pagination, {
      page: { total: 1, offset: 60, limit: 60 },
      query: "",
      path: "/artists/credit",
      parameter: "recording_offset",
      label: "Track pages",
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Previous" }));
  expect(navigation.get).toHaveBeenCalledWith("/artists/credit", {
    offset: "4",
    recording_offset: 0,
  });
});
