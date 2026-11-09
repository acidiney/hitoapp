import { createElement } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import { Playback } from "../playback";
import NowPlaying from "../components/NowPlaying";
import { QueueMedia, queueTrack } from "../fixtures/browserQueueMedia";

let player: Playback;
const close = vi.fn();
vi.mock("@inertiajs/react", () => ({ router: { visit: vi.fn() } }));
vi.mock("../components/PlaybackProvider", () => ({ usePlayer: () => player }));
vi.mock("../../artwork/components/Artwork", () => ({
  default: () => createElement("div"),
}));

beforeEach(async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => Response.json({ installations: [], services: [] })),
  );
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
  player = new Playback(new QueueMedia(), async (id) => [{ id, ready: true }]);
  await player.playQueue([
    {
      ...queueTrack("duo", "Duet"),
      artist: "Duo A & Duo B",
      credited_artists: [
        { id: "member-a", name: "Duo A", role: "main", join: " & " },
        { id: "member-b", name: "Duo B", role: "main" },
      ],
    },
  ]);
  render(
    createElement(NowPlaying, { account: "alice", close, openQueue: close }),
  );
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  close.mockClear();
});

test("each credited artist links to their page and closes the full-screen player", () => {
  expect(screen.getByRole("link", { name: "Duo A" }).getAttribute("href")).toBe(
    "/artists/member-a",
  );
  const second = screen.getByRole("link", { name: "Duo B" });
  expect(second.getAttribute("href")).toBe("/artists/member-b");
  fireEvent.click(second);
  expect(close).toHaveBeenCalledTimes(1);
  expect(vi.mocked(router.visit)).toHaveBeenCalledWith(
    "/artists/member-b",
    expect.anything(),
  );
});
