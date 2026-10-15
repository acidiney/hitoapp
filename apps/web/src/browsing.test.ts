// @vitest-environment jsdom
import { createElement, type AnchorHTMLAttributes } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import AlbumPage from "./pages/AlbumPage";
import Pagination from "./components/Pagination";

const player = vi.hoisted(() => ({
  playQueue: vi.fn(),
  subscribe: () => () => {},
  snapshot: () => ({ current: null }),
}));
vi.mock("./player/PlaybackProvider", () => ({ usePlayer: () => player }));
vi.mock("./hooks/useMutation", () => ({
  useMutation: () => ({ mutate: vi.fn(), pending: false }),
}));
const navigation = vi.hoisted(() => ({ get: vi.fn() }));
vi.mock("@inertiajs/react", () => ({
  router: navigation,
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));
afterEach(cleanup);

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
