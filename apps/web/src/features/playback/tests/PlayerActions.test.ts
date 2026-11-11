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
import type { Schema } from "../../../shared/schema";
import { Playback } from "../playback";
import PlayerDock from "../components/PlayerDock";
import NowPlaying from "../components/NowPlaying";
import QueueList from "../components/QueueList";
import { TrackActionsProvider } from "../components/TrackActionsProvider";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { QueueMedia, queueTrack } from "../fixtures/browserQueueMedia";
import type { Track } from "../model";

let player: Playback;
const change = vi.hoisted(() => vi.fn(async () => {}));
const router = vi.hoisted(() => ({ visit: vi.fn(), reload: vi.fn() }));
vi.mock("../components/PlaybackProvider", () => ({ usePlayer: () => player }));
vi.mock("../../artwork/components/Artwork", () => ({ default: () => null }));
vi.mock("../../../shared/sync", () => ({ change }));
vi.mock("@inertiajs/react", () => ({
  router,
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

const playlists = [
  { id: "late", name: "Late night", kind: "local" },
  { id: "feed", name: "Followed", kind: "followed" },
] as Schema<"Playlist">[];

const first: Track = {
  ...queueTrack("r0", "Salt Flats"),
  artist: "Cartographers",
  artist_id: "artist-1",
  release_id: "release-1",
  liked: false,
};

function shell(child: ReturnType<typeof createElement>) {
  return createElement(NoticeProvider, {
    children: createElement(TrackActionsProvider, {
      account: "alice",
      playlists,
      children: child,
    }),
  });
}

beforeEach(async () => {
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => Response.json({ items: [], installations: [] })),
  );
  player = new Playback(new QueueMedia(), async (id) => [{ id, ready: true }]);
  await act(async () => {
    await player.playQueue([first, queueTrack("r1", "Next up")]);
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  change.mockClear();
  router.visit.mockClear();
  router.reload.mockClear();
});

async function openMenu(scope: ReturnType<typeof within>, title: string) {
  fireEvent.click(scope.getByRole("button", { name: `Actions for ${title}` }));
  return screen.findByRole("menu", { name: `Actions for ${title}` });
}

test("the player bar likes the playing track and keeps the heart in step", async () => {
  render(shell(createElement(PlayerDock, { account: "alice" })));
  const dock = within(screen.getByRole("contentinfo"));
  fireEvent.click(dock.getByRole("button", { name: "Like Salt Flats" }));
  await waitFor(() =>
    expect(
      dock
        .getByRole("button", { name: "Unlike Salt Flats" })
        .getAttribute("aria-pressed"),
    ).toBe("true"),
  );
  expect(change).toHaveBeenCalledWith(
    "alice",
    { kind: "like_set", recording_id: "r0", liked: true },
    expect.anything(),
  );
  const queued = await player.queuePage(0, 1);
  expect(queued.entries[0].recording.liked).toBe(true);
});

test("the player bar menu offers details, playlists and source but not queue actions", async () => {
  render(shell(createElement(PlayerDock, { account: "alice" })));
  const dock = within(screen.getByRole("contentinfo"));
  const menu = await openMenu(dock, "Salt Flats");
  expect(
    within(menu)
      .getAllByRole("menuitem")
      .map((item) => item.textContent),
  ).toEqual(["Track details", "Add to playlist", "Fix source"]);
  fireEvent.click(
    within(menu).getByRole("menuitem", { name: "Add to playlist" }),
  );
  const submenu = await screen.findByRole("menu", { name: "Add to playlist" });
  expect(
    within(submenu)
      .getAllByRole("menuitem")
      .map((item) => item.textContent),
  ).toEqual(["Add to playlist", "Late night"]);
  fireEvent.click(
    within(submenu).getByRole("menuitem", { name: "Late night" }),
  );
  await waitFor(() =>
    expect(change).toHaveBeenCalledWith(
      "alice",
      expect.objectContaining({
        kind: "entry_insert",
        playlist_id: "late",
        recording_id: "r0",
      }),
      expect.anything(),
    ),
  );
});

test("the player bar menu opens the track page", async () => {
  render(shell(createElement(PlayerDock, { account: "alice" })));
  const menu = await openMenu(
    within(screen.getByRole("contentinfo")),
    "Salt Flats",
  );
  fireEvent.click(
    within(menu).getByRole("menuitem", { name: "Track details" }),
  );
  expect(router.visit).toHaveBeenCalledWith("/recordings/r0");
});

test("the artist links to the artist page when its id is known", () => {
  render(shell(createElement(PlayerDock, { account: "alice" })));
  expect(
    screen.getByRole("link", { name: "Cartographers" }).getAttribute("href"),
  ).toBe("/artists/artist-1");
  expect(
    screen.getByRole("link", { name: "Salt Flats" }).getAttribute("href"),
  ).toBe("/albums/release-1");
});

test("the player bar shows no actions without a playing track", () => {
  player = new Playback(new QueueMedia(), async () => []);
  render(shell(createElement(PlayerDock, { account: "alice" })));
  expect(screen.queryByRole("button", { name: /^Like / })).toBeNull();
  expect(screen.queryByRole("button", { name: /^Actions for/ })).toBeNull();
});

test("now playing has the heart and menu and leaves the dialog for the track page", async () => {
  const close = vi.fn();
  render(
    shell(
      createElement(NowPlaying, {
        account: "alice",
        close,
        openQueue: vi.fn(),
      }),
    ),
  );
  const dialog = within(screen.getByRole("dialog", { name: "Now playing" }));
  expect(dialog.getByRole("button", { name: "Like Salt Flats" })).toBeTruthy();
  expect(
    dialog.getByRole("link", { name: "Cartographers" }).getAttribute("href"),
  ).toBe("/artists/artist-1");
  const menu = await openMenu(dialog, "Salt Flats");
  expect(menu.closest("dialog")).not.toBeNull();
  fireEvent.click(
    within(menu).getByRole("menuitem", { name: "Track details" }),
  );
  expect(close).toHaveBeenCalled();
  expect(router.visit).toHaveBeenCalledWith("/recordings/r0");
});

test("every queue row has a menu without queue actions and no heart", async () => {
  render(shell(createElement(QueueList, { from: 1 })));
  const row = await screen.findByText("Next up");
  const item = within(row.closest("li")!);
  expect(item.queryByRole("button", { name: "Like Next up" })).toBeNull();
  fireEvent.click(
    item.getByRole("button", {
      name: "Actions for Next up: move up, move down, remove",
    }),
  );
  expect(
    screen.getAllByRole("menuitem").map((entry) => entry.textContent),
  ).toEqual([
    "Move up",
    "Move down",
    "Remove",
    "Track details",
    "Add to playlist",
    "Fix source",
  ]);
});

test("without the actions provider the player bar renders no actions", () => {
  render(createElement(PlayerDock, { account: "alice" }));
  expect(screen.queryByRole("button", { name: /^Like / })).toBeNull();
});
