import { createElement } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { Playback } from "../playback";
import NowPlaying from "../components/NowPlaying";
import { QueueMedia, queueTrack } from "../fixtures/browserQueueMedia";
import { sourceStatusStore } from "../../sources/model/sourceStatusStore";

let player: Playback;
const fetch = vi.fn(async () => Response.json({ items: [] }));

vi.mock("@inertiajs/react", () => ({ router: { visit: vi.fn() } }));
vi.mock("../components/PlaybackProvider", () => ({ usePlayer: () => player }));
vi.mock("../../artwork/components/Artwork", () => ({
  default: () => createElement("div"),
}));

beforeEach(async () => {
  sourceStatusStore.reset();
  vi.stubGlobal("fetch", fetch);
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: query.includes("max-width: 767px"),
    media: query,
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
  player = new Playback(new QueueMedia(), async (id) => [{ id, ready: true }]);
  await player.playQueue([queueTrack("r0", "Track 0")]);
  await act(async () => {
    render(
      createElement(NowPlaying, {
        account: "alice",
        close: vi.fn(),
        openQueue: vi.fn(),
      }),
    );
  });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test("a phone shows the player with the Lyrics, Up next and Source tabs under it", () => {
  expect(
    screen.getByRole("heading", { level: 1, name: "Track 0" }),
  ).toBeTruthy();
  const tabs = screen.getByRole("group", { name: "Now playing panels" });
  expect(within(tabs).getByRole("button", { name: "Lyrics" })).toBeTruthy();
  expect(within(tabs).getByRole("link", { name: "Up next" })).toBeTruthy();
  expect(within(tabs).getByRole("button", { name: "Source" })).toBeTruthy();
  expect(screen.queryByRole("region", { name: "Lyrics" })).toBeNull();
  expect(screen.queryByRole("region", { name: "This source" })).toBeNull();
});

test("a phone opens a panel beside a compact player and returns to the full player", async () => {
  const tabs = screen.getByRole("group", { name: "Now playing panels" });
  await act(async () => {
    fireEvent.click(within(tabs).getByRole("button", { name: "Lyrics" }));
  });
  expect(screen.queryByRole("heading", { level: 1 })).toBeNull();
  expect(screen.getByRole("region", { name: "Lyrics" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Pause" })).toBeTruthy();
  await act(async () => {
    fireEvent.click(
      within(
        screen.getByRole("group", { name: "Now playing panels" }),
      ).getByRole("button", { name: "Source" }),
    );
  });
  expect(screen.getByRole("region", { name: "This source" })).toBeTruthy();
  expect(screen.queryByRole("region", { name: "Lyrics" })).toBeNull();
  await act(async () => {
    fireEvent.click(
      screen.getByRole("button", { name: "Close panel, back to player" }),
    );
  });
  expect(
    screen.getByRole("heading", { level: 1, name: "Track 0" }),
  ).toBeTruthy();
  expect(screen.queryByRole("region", { name: "This source" })).toBeNull();
});
