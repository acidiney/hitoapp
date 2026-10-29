import { createElement } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { Playback } from "../playback";
import QueueScreen from "../components/QueueScreen";
import { QueueMedia, queueTrack } from "../fixtures/browserQueueMedia";

let player: Playback;
vi.mock("../components/PlaybackProvider", () => ({ usePlayer: () => player }));
vi.mock("../../artwork/components/Artwork", () => ({ default: () => null }));

beforeEach(async () => {
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
  player = new Playback(new QueueMedia(), async (id) => [{ id, ready: true }]);
  await player.playQueue(
    Array.from({ length: 5 }, (_, index) =>
      queueTrack(`r${index}`, `Track ${index}`),
    ),
    1,
  );
  render(createElement(QueueScreen));
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const upNext = () =>
  [...document.querySelectorAll("ol.queue-tracks > li")].map(
    (row) => row.querySelector("strong")!.textContent,
  );

test("the header counts the tracks and the playing track has its own row", async () => {
  expect(screen.getByRole("heading", { name: "Queue", level: 1 })).toBeTruthy();
  expect(screen.getByText("5 tracks")).toBeTruthy();
  const now = screen.getByRole("region", { name: "Now playing" });
  expect(within(now).getByText("Track 1")).toBeTruthy();
  await waitFor(() =>
    expect(upNext()).toEqual(["Track 2", "Track 3", "Track 4"]),
  );
  expect(
    screen.getByRole("heading", { name: "Up next", level: 2 }),
  ).toBeTruthy();
});

test("moving a track with the keyboard buttons reorders only the upcoming list", async () => {
  await waitFor(() => expect(upNext().length).toBe(3));
  fireEvent.click(screen.getByRole("button", { name: "Move Track 4 up" }));
  await waitFor(() =>
    expect(upNext()).toEqual(["Track 2", "Track 4", "Track 3"]),
  );
  expect(
    screen.getByRole("button", { name: "Move Track 2 up" }),
  ).toHaveProperty("disabled", true);
});

test("a track can be removed from the queue", async () => {
  await waitFor(() => expect(upNext().length).toBe(3));
  fireEvent.click(
    screen.getByRole("button", { name: "Remove Track 3 from queue" }),
  );
  await waitFor(() => expect(upNext()).toEqual(["Track 2", "Track 4"]));
  expect(screen.getByText("4 tracks")).toBeTruthy();
});

test("Clear asks first and then empties the queue", async () => {
  fireEvent.click(screen.getByRole("button", { name: "Clear" }));
  const dialog = screen.getByRole("dialog");
  expect(player.queueSnapshot().total).toBe(5);
  fireEvent.click(within(dialog).getByRole("button", { name: "Clear queue" }));
  await waitFor(() => expect(player.queueSnapshot().total).toBe(0));
  expect(screen.getByText(/Choose a track to start listening/)).toBeTruthy();
});

test("Save as playlist is not offered without a capability", () => {
  expect(screen.queryByRole("button", { name: /Save as playlist/ })).toBeNull();
});
