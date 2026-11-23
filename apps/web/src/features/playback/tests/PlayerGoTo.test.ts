import { createElement, type AnchorHTMLAttributes } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { Playback } from "../playback";
import PlayerDock from "../components/PlayerDock";
import NowPlaying from "../components/NowPlaying";
import { TrackActionsProvider } from "../components/TrackActionsProvider";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { QueueMedia, queueTrack } from "../fixtures/browserQueueMedia";
import type { Track } from "../model";

let player: Playback;
const router = vi.hoisted(() => ({ visit: vi.fn(), reload: vi.fn() }));
vi.mock("../components/PlaybackProvider", () => ({ usePlayer: () => player }));
vi.mock("../../artwork/components/Artwork", () => ({ default: () => null }));
vi.mock("../../../shared/sync", () => ({ change: vi.fn(async () => {}) }));
vi.mock("@inertiajs/react", () => ({
  router,
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

const known: Track = {
  ...queueTrack("go-known", "Salt Flats"),
  artist: "Cartographers",
  artist_id: "artist-1",
  release_id: "release-1",
};

function identity(recording: string, group: string | null) {
  return {
    recording_id: recording,
    revision: 1,
    state: group
      ? { status: "selected", group_id: group, association_id: "a" }
      : { status: "unselected" },
  };
}

async function playing(track: Track, matched: string | null = null) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL) =>
      String(input).includes("/artist-identity")
        ? Response.json(identity(track.id, matched))
        : Response.json({ items: [], installations: [], services: [] }),
    ),
  );
  player = new Playback(new QueueMedia(), async (id) => [{ id, ready: true }]);
  await act(async () => {
    await player.playQueue([track]);
  });
}

function shell(child: ReturnType<typeof createElement>) {
  return createElement(NoticeProvider, {
    children: createElement(TrackActionsProvider, {
      account: "alice",
      playlists: [],
      children: child,
    }),
  });
}

function nowPlaying(close = vi.fn()) {
  render(
    shell(
      createElement(NowPlaying, {
        account: "alice",
        close,
        openQueue: vi.fn(),
      }),
    ),
  );
  return { close, dialog: within(screen.getByRole("dialog")) };
}

async function openMenu(scope: ReturnType<typeof within>, title: string) {
  fireEvent.click(scope.getByRole("button", { name: `Actions for ${title}` }));
  return screen.findByRole("menu", { name: `Actions for ${title}` });
}

function labels(menu: HTMLElement) {
  return within(menu)
    .getAllByRole("menuitem")
    .map((item) => item.textContent);
}

beforeEach(() => {
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  router.visit.mockClear();
});

test("the player bar menu goes to the playing track's artist and album", async () => {
  await playing(known);
  render(shell(createElement(PlayerDock, { account: "alice" })));
  const menu = await openMenu(
    within(screen.getByRole("contentinfo")),
    "Salt Flats",
  );
  expect(labels(menu)).toEqual([
    "Go to artist",
    "Go to album",
    "Track details",
    "Add to playlist",
    "Fix source",
  ]);
  fireEvent.click(within(menu).getByRole("menuitem", { name: "Go to album" }));
  expect(router.visit).toHaveBeenCalledWith(
    "/albums/release-1",
    expect.anything(),
  );
});

test("now playing closes before going to the artist from its menu", async () => {
  await playing(known);
  const { close, dialog } = nowPlaying();
  const menu = await openMenu(dialog, "Salt Flats");
  fireEvent.click(within(menu).getByRole("menuitem", { name: "Go to artist" }));
  expect(close).toHaveBeenCalledTimes(1);
  expect(router.visit).toHaveBeenCalledWith(
    "/artists/artist-1",
    expect.anything(),
  );
  expect(close.mock.invocationCallOrder[0]).toBeLessThan(
    router.visit.mock.invocationCallOrder[0],
  );
});

test("the now playing title opens the album and closes the full-screen player", async () => {
  await playing(known);
  const { close, dialog } = nowPlaying();
  const title = dialog.getByRole("link", { name: "Salt Flats" });
  expect(title.getAttribute("href")).toBe("/albums/release-1");
  fireEvent.click(title);
  expect(close).toHaveBeenCalledTimes(1);
  expect(router.visit).toHaveBeenCalledWith(
    "/albums/release-1",
    expect.anything(),
  );
});

test("a track crediting several artists lets you choose which one to open", async () => {
  await playing({
    ...known,
    artist: "Duo A & Duo B",
    credited_artists: [
      { id: "member-a", name: "Duo A", role: "main", join: " & " },
      { id: "member-b", name: "Duo B", role: "featured" },
    ],
  });
  const { close, dialog } = nowPlaying();
  const menu = await openMenu(dialog, "Salt Flats");
  fireEvent.click(within(menu).getByRole("menuitem", { name: "Go to artist" }));
  const chooser = await screen.findByRole("menu", { name: "Go to artist" });
  expect(labels(chooser)).toEqual(["Go to artist", "Duo A", "Duo B"]);
  fireEvent.click(within(chooser).getByRole("menuitem", { name: "Duo B" }));
  expect(close).toHaveBeenCalledTimes(1);
  expect(router.visit).toHaveBeenCalledWith(
    "/artists/member-b",
    expect.anything(),
  );
});

test("a service track without a library artist or album goes to the matched artist and hides the album", async () => {
  await playing(
    { ...known, id: "go-service", artist_id: null, release_id: null },
    "group-1",
  );
  const { dialog } = nowPlaying();
  await waitFor(() =>
    expect(
      dialog.getByRole("link", { name: "Cartographers" }).getAttribute("href"),
    ).toBe("/artist-groups/group-1"),
  );
  const menu = await openMenu(dialog, "Salt Flats");
  expect(labels(menu)).toEqual([
    "Go to artist",
    "Track details",
    "Add to playlist",
    "Fix source",
  ]);
  fireEvent.click(within(menu).getByRole("menuitem", { name: "Go to artist" }));
  expect(router.visit).toHaveBeenCalledWith(
    "/artist-groups/group-1",
    expect.anything(),
  );
});

test("a service track with no artist anywhere hides both options", async () => {
  await playing({
    ...known,
    id: "go-unmatched",
    artist_id: null,
    release_id: null,
  });
  const { dialog } = nowPlaying();
  const menu = await openMenu(dialog, "Salt Flats");
  await waitFor(() =>
    expect(labels(menu)).toEqual([
      "Track details",
      "Add to playlist",
      "Fix source",
    ]),
  );
});
