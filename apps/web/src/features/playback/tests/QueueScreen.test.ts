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

const rowMenu = (title: string) =>
  screen.getByRole("button", {
    name: `Actions for ${title}: move up, move down, remove`,
  });
function chooseItem(title: string, item: string) {
  fireEvent.click(rowMenu(title));
  fireEvent.click(screen.getByRole("menuitem", { name: item }));
}

const upNext = () =>
  [...document.querySelectorAll("ol.queue-tracks > li")].map(
    (row) => row.querySelector("strong")!.textContent,
  );

test("the header counts the tracks and the playing track has its own row", async () => {
  expect(screen.getByRole("heading", { name: "Queue", level: 1 })).toBeTruthy();
  expect(screen.getByText("5 tracks · 3 up next")).toBeTruthy();
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
  chooseItem("Track 4", "Move up");
  await waitFor(() =>
    expect(upNext()).toEqual(["Track 2", "Track 4", "Track 3"]),
  );
  fireEvent.click(rowMenu("Track 2"));
  expect(screen.getByRole("menuitem", { name: "Move up" })).toHaveProperty(
    "disabled",
    true,
  );
});

test("a track can be removed from the queue", async () => {
  await waitFor(() => expect(upNext().length).toBe(3));
  chooseItem("Track 3", "Remove");
  await waitFor(() => expect(upNext()).toEqual(["Track 2", "Track 4"]));
  expect(screen.getByText("4 tracks · 2 up next")).toBeTruthy();
});

test("Clear asks first and then empties the queue", async () => {
  fireEvent.click(screen.getByRole("button", { name: "Clear" }));
  const dialog = screen.getByRole("dialog");
  expect(player.queueSnapshot().total).toBe(5);
  fireEvent.click(within(dialog).getByRole("button", { name: "Clear queue" }));
  await waitFor(() => expect(player.queueSnapshot().total).toBe(0));
  expect(screen.getByText(/Choose a track to start listening/)).toBeTruthy();
});

test("cancelling Clear keeps the queue and returns focus to the Clear button", async () => {
  const clear = screen.getByRole("button", { name: "Clear" });
  clear.focus();
  fireEvent.click(clear);
  fireEvent.click(
    within(screen.getByRole("dialog")).getByRole("button", { name: "Cancel" }),
  );
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(player.queueSnapshot().total).toBe(5);
  expect(document.activeElement).toBe(clear);
});

test("played tracks stay reachable in a collapsed section and can be removed", async () => {
  const played = screen.getByText("Played (1)").closest("details")!;
  expect(played.open).toBe(false);
  fireEvent.click(screen.getByText("Played (1)"));
  await waitFor(() => expect(within(played).getByText("Track 0")).toBeTruthy());
  fireEvent.click(
    within(played).getByRole("button", {
      name: "Actions for Track 0: move up, move down, remove",
    }),
  );
  fireEvent.click(screen.getByRole("menuitem", { name: "Remove" }));
  await waitFor(() => expect(player.queueSnapshot().total).toBe(4));
  expect(screen.queryByText(/^Played/)).toBeNull();
});

test("the playing track can be removed from its own row", async () => {
  fireEvent.click(
    within(screen.getByRole("region", { name: "Now playing" })).getByRole(
      "button",
      { name: "Remove Track 1 from queue" },
    ),
  );
  await waitFor(() => expect(player.queueSnapshot().total).toBe(4));
});

test("the heading separates the total from what is up next", () => {
  expect(screen.getByText("5 tracks · 3 up next")).toBeTruthy();
});

test("Save as playlist is not offered without a capability", () => {
  expect(screen.queryByRole("button", { name: /Save as playlist/ })).toBeNull();
});
