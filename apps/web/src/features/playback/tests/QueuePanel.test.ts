import { createElement } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { Playback } from "../playback";
import QueuePanel from "../components/QueuePanel";
import {
  QUEUE_PANEL_ID,
  resetQueuePanelForTests,
  setQueuePanelOpen,
  toggleQueuePanel,
} from "../queuePanelState";
import { QueueMedia, queueTrack } from "../fixtures/browserQueueMedia";

let player: Playback;
vi.mock("../components/PlaybackProvider", () => ({ usePlayer: () => player }));
vi.mock("../../artwork/components/Artwork", () => ({ default: () => null }));

beforeEach(async () => {
  window.localStorage.clear();
  resetQueuePanelForTests();
  player = new Playback(new QueueMedia(), async (id) => [{ id, ready: true }]);
  await player.playQueue(
    Array.from({ length: 4 }, (_, index) =>
      queueTrack(`r${index}`, `Track ${index}`),
    ),
    0,
  );
});

afterEach(() => {
  cleanup();
  window.localStorage.clear();
  resetQueuePanelForTests();
});

function shown() {
  return document.getElementById(QUEUE_PANEL_ID)!;
}

test("the panel is hidden and inert until it is opened and then shows now playing and up next", async () => {
  render(createElement(QueuePanel));
  expect(shown().hasAttribute("data-open")).toBe(false);
  expect(shown().hasAttribute("inert")).toBe(true);
  act(() => toggleQueuePanel());
  expect(shown().hasAttribute("data-open")).toBe(true);
  expect(shown().hasAttribute("inert")).toBe(false);
  expect(screen.getByRole("complementary", { name: "Queue" })).toBeTruthy();
  expect(screen.getByRole("heading", { name: "Queue" })).toBeTruthy();
  expect(screen.getByRole("region", { name: "Now playing" })).toBeTruthy();
  await waitFor(() =>
    expect(
      screen.getByRole("region", { name: "Up next" }).querySelectorAll("li"),
    ).toHaveLength(3),
  );
});

test("the open state is remembered on this device and survives a reload", () => {
  act(() => setQueuePanelOpen(true));
  expect(window.localStorage.getItem("hito.queue-panel")).toBe("open");
  act(() => resetQueuePanelForTests());
  render(createElement(QueuePanel));
  expect(shown().hasAttribute("data-open")).toBe(true);
  act(() => setQueuePanelOpen(false));
  expect(window.localStorage.getItem("hito.queue-panel")).toBe("closed");
});

test("storage that throws does not break the toggle", () => {
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  render(createElement(QueuePanel));
  act(() => toggleQueuePanel());
  expect(shown().hasAttribute("data-open")).toBe(true);
  vi.restoreAllMocks();
});

test("opening moves focus into the panel and Escape closes it and returns focus to the Queue button", () => {
  const toggle = document.createElement("button");
  toggle.setAttribute("data-queue-toggle", "");
  document.body.append(toggle);
  render(createElement(QueuePanel));
  act(() => toggleQueuePanel());
  const heading = screen.getByRole("heading", { name: "Queue" });
  expect(document.activeElement).toBe(heading);
  fireEvent.keyDown(heading, { key: "Escape" });
  expect(shown().hasAttribute("data-open")).toBe(false);
  expect(document.activeElement).toBe(toggle);
  toggle.remove();
});

test("the close button closes the panel", () => {
  act(() => setQueuePanelOpen(true));
  render(createElement(QueuePanel));
  fireEvent.click(screen.getByRole("button", { name: "Close queue" }));
  expect(shown().hasAttribute("data-open")).toBe(false);
});
