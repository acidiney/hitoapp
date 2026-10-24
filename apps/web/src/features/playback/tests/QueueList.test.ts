import { createElement } from "react";
import {
  act,
  cleanup,
  createEvent,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { Playback } from "../playback";
import QueueList from "../components/QueueList";
import type { QueueSupply } from "../model";
import { QueueMedia, queueTrack } from "../fixtures/browserQueueMedia";

let player: Playback;
vi.mock("../components/PlaybackProvider", () => ({ usePlayer: () => player }));
vi.mock("../../artwork/components/Artwork", () => ({ default: () => null }));
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

async function listed(count: number, index = 0) {
  const media = new QueueMedia();
  player = new Playback(media, async (id) => [{ id, ready: true }]);
  await player.playQueue(
    Array.from({ length: count }, (_, i) => queueTrack(`r${i}`, `Track ${i}`)),
    index,
  );
  render(createElement(QueueList));
  return media;
}
const rows = () => [...document.querySelectorAll("ol.queue-tracks > li")];
const titles = () =>
  rows().map((row) => row.querySelector("strong")!.textContent);
function dragAt(type: "dragOver" | "drop", target: Element, clientY: number) {
  const event = createEvent[type](target, {
    dataTransfer: { setData: vi.fn(), effectAllowed: "" },
  });
  Object.defineProperty(event, "clientY", { value: clientY });
  fireEvent(target, event);
}
function scrollTo(top: number) {
  const window = document.querySelector(".queue-window")!;
  Object.defineProperty(window, "scrollTop", {
    value: top,
    configurable: true,
  });
  fireEvent.scroll(window);
}

test("ten thousand queued entries render only the visible window", async () => {
  await listed(10_000);
  await waitFor(() => expect(rows().length).toBeGreaterThan(0));
  expect(rows().length).toBeLessThanOrEqual(40);
  expect(titles()[0]).toBe("Track 0");
  expect(screen.queryByText("Track 5000")).toBeNull();
  expect(rows()[0].getAttribute("aria-setsize")).toBe("10000");

  scrollTo(5000 * 56);
  await waitFor(() => expect(screen.queryByText("Track 5000")).not.toBeNull());
  expect(rows().length).toBeLessThanOrEqual(40);
  expect(screen.queryByText("Track 0")).toBeNull();
  const list = document.querySelector("ol.queue-tracks") as HTMLElement;
  expect(list.style.height).toBe(`${10_000 * 56}px`);
});

test("the window opens at the playing entry", async () => {
  await listed(10_000, 7_000);
  await waitFor(() => expect(screen.queryByText("Track 7000")).not.toBeNull());
  expect(document.querySelector("li.current strong")?.textContent).toBe(
    "Track 7000",
  );
  expect(rows().length).toBeLessThanOrEqual(40);
});

test("position ticks neither fetch queue pages nor re-render queue rows", async () => {
  const media = await listed(1_000);
  await waitFor(() => expect(rows().length).toBeGreaterThan(0));
  const page = vi.spyOn(player, "queuePage");
  const first = rows()[0];
  for (let tick = 1; tick <= 20; tick++)
    act(() => {
      media.currentTime = tick;
      media.dispatchEvent(new Event("timeupdate"));
    });
  expect(page).not.toHaveBeenCalled();
  expect(rows()[0]).toBe(first);
});

test("move buttons reorder by one place without restarting playback", async () => {
  const media = await listed(6, 1);
  await waitFor(() => expect(rows()).toHaveLength(6));
  const loads = media.loads.length;
  fireEvent.click(screen.getByRole("button", { name: "Move Track 3 up" }));
  await waitFor(() =>
    expect(titles()).toEqual([
      "Track 0",
      "Track 1",
      "Track 3",
      "Track 2",
      "Track 4",
      "Track 5",
    ]),
  );
  fireEvent.click(screen.getByRole("button", { name: "Move Track 0 down" }));
  await waitFor(() =>
    expect(titles().slice(0, 2)).toEqual(["Track 1", "Track 0"]),
  );
  fireEvent.click(screen.getByRole("button", { name: "Move Track 5 up" }));
  await waitFor(() =>
    expect(titles().slice(-2)).toEqual(["Track 5", "Track 4"]),
  );
  expect(
    screen
      .getByRole("button", { name: "Move Track 1 up" })
      .hasAttribute("disabled"),
  ).toBe(true);
  expect(
    screen
      .getByRole("button", { name: "Move Track 4 down" })
      .hasAttribute("disabled"),
  ).toBe(true);
  expect(player.snapshot().current?.title).toBe("Track 1");
  expect(media.loads).toHaveLength(loads);
  expect(document.querySelector("[aria-live=polite]")!.textContent).toMatch(
    /Track 5.*5 of 6/,
  );
});

test("dragging a row onto another drops it before or after that row", async () => {
  await listed(5);
  await waitFor(() => expect(rows()).toHaveLength(5));
  const dataTransfer = { setData: vi.fn(), effectAllowed: "" };
  const handle = (title: string) =>
    screen.getByRole("button", { name: `Drag ${title}` });
  fireEvent.dragStart(handle("Track 0"), { dataTransfer });
  dragAt("dragOver", rows()[3], 10);
  dragAt("drop", rows()[3], 10);
  await waitFor(() =>
    expect(titles()).toEqual([
      "Track 1",
      "Track 2",
      "Track 3",
      "Track 0",
      "Track 4",
    ]),
  );
  fireEvent.dragStart(handle("Track 4"), { dataTransfer });
  dragAt("dragOver", rows()[0], -10);
  dragAt("drop", rows()[0], -10);
  await waitFor(() => expect(titles()[0]).toBe("Track 4"));
});

test("dropping on the neighbouring slot changes nothing", async () => {
  await listed(3);
  await waitFor(() => expect(rows()).toHaveLength(3));
  const version = player.queueSnapshot().version;
  const dataTransfer = { setData: vi.fn(), effectAllowed: "" };
  fireEvent.dragStart(screen.getByRole("button", { name: "Drag Track 1" }), {
    dataTransfer,
  });
  dragAt("dragOver", rows()[1], 10);
  dragAt("drop", rows()[1], 10);
  await Promise.resolve();
  expect(player.queueSnapshot().version).toBe(version);
});

test("entries still arriving are announced", async () => {
  const media = new QueueMedia();
  player = new Playback(media, async (id) => [{ id, ready: true }]);
  const more: QueueSupply = {
    remaining: () => 4200,
    next: () => new Promise(() => {}),
  };
  await player.playQueue([queueTrack("r0", "Track 0")], 0, undefined, more);
  render(createElement(QueueList));
  await waitFor(() =>
    expect(screen.getByText(/4,200 more tracks/)).not.toBeNull(),
  );
});
