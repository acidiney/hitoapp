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
import { QUEUE_ROW } from "../hooks/useQueueWindow";
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
function menuFor(title: string) {
  return screen.getByRole("button", {
    name: `Actions for ${title}: move up, move down, remove`,
  });
}
function choose(title: string, item: string) {
  fireEvent.click(menuFor(title));
  fireEvent.click(screen.getByRole("menuitem", { name: item }));
}
function grip(title: string) {
  return screen.getByRole("button", {
    name: new RegExp(
      `^Reorder ${title}: drag, or press (Option|Alt) and Up or Down$`,
    ),
  });
}
function pointer(type: string, target: Element, clientY: number) {
  const event = createEvent(type, target, { bubbles: true, cancelable: true });
  Object.defineProperties(event, {
    clientY: { value: clientY },
    pointerId: { value: 1 },
    pointerType: { value: "mouse" },
    button: { value: 0 },
  });
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

  scrollTo(5000 * QUEUE_ROW);
  await waitFor(() => expect(screen.queryByText("Track 5000")).not.toBeNull());
  expect(rows().length).toBeLessThanOrEqual(40);
  expect(screen.queryByText("Track 0")).toBeNull();
  const list = document.querySelector("ol.queue-tracks") as HTMLElement;
  expect(list.style.height).toBe(`${10_000 * QUEUE_ROW}px`);
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

test("the row menu moves a track by one place without restarting playback", async () => {
  const media = await listed(6, 1);
  await waitFor(() => expect(rows()).toHaveLength(6));
  const loads = media.loads.length;
  choose("Track 3", "Move up");
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
  choose("Track 0", "Move down");
  await waitFor(() =>
    expect(titles().slice(0, 2)).toEqual(["Track 1", "Track 0"]),
  );
  choose("Track 5", "Move up");
  await waitFor(() =>
    expect(titles().slice(-2)).toEqual(["Track 5", "Track 4"]),
  );
  expect(player.snapshot().current?.title).toBe("Track 1");
  expect(media.loads).toHaveLength(loads);
  expect(document.querySelector("[aria-live=polite]")!.textContent).toMatch(
    /Track 5.*5 of 6/,
  );
});

test("the row menu offers move up, move down and remove, and disables moves at the ends", async () => {
  await listed(3);
  await waitFor(() => expect(rows()).toHaveLength(3));
  fireEvent.click(menuFor("Track 0"));
  const names = screen.getAllByRole("menuitem").map((item) => item.textContent);
  expect(names.slice(0, 3)).toEqual(["Move up", "Move down", "Remove"]);
  expect(
    (screen.getByRole("menuitem", { name: "Move up" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
  fireEvent.click(menuFor("Track 2"));
  expect(
    (screen.getByRole("menuitem", { name: "Move down" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
});

test("Remove in the row menu takes the track out of the queue", async () => {
  await listed(4);
  await waitFor(() => expect(rows()).toHaveLength(4));
  choose("Track 2", "Remove");
  await waitFor(() =>
    expect(titles()).toEqual(["Track 0", "Track 1", "Track 3"]),
  );
});

test("a row has one grip and one actions button and no separate move or remove buttons", async () => {
  await listed(2);
  await waitFor(() => expect(rows()).toHaveLength(2));
  const buttons = [...rows()[1].querySelectorAll("button")].map((button) =>
    button.getAttribute("aria-label"),
  );
  expect(buttons).toEqual([
    expect.stringMatching(
      /^Reorder Track 1: drag, or press (Option|Alt) and Up or Down$/,
    ),
    null,
    "Actions for Track 1: move up, move down, remove",
  ]);
  expect(menuFor("Track 1").getAttribute("aria-haspopup")).toBe("menu");
  expect(grip("Track 1").hasAttribute("title")).toBe(false);
});

test("Alt with an arrow moves the focused row by one place", async () => {
  await listed(5);
  await waitFor(() => expect(rows()).toHaveLength(5));
  const row = (title: string) =>
    rows().find((item) => item.querySelector("strong")!.textContent === title)!;
  const handle = row("Track 3").querySelector("button")!;
  fireEvent.keyDown(handle, { key: "ArrowUp", altKey: true });
  await waitFor(() =>
    expect(titles()).toEqual([
      "Track 0",
      "Track 1",
      "Track 3",
      "Track 2",
      "Track 4",
    ]),
  );
  fireEvent.keyDown(row("Track 4").querySelector("button")!, {
    key: "ArrowDown",
    altKey: true,
  });
  expect(titles().at(-1)).toBe("Track 4");
  fireEvent.keyDown(row("Track 0").querySelector("button")!, {
    key: "ArrowUp",
    altKey: true,
  });
  expect(titles()[0]).toBe("Track 0");
  fireEvent.keyDown(row("Track 1").querySelector("button")!, {
    key: "ArrowDown",
  });
  expect(titles()[1]).toBe("Track 1");
});

test("after Alt with an arrow the grip of the moved row keeps focus", async () => {
  await listed(5, 4);
  await waitFor(() => expect(rows()).toHaveLength(5));
  grip("Track 1").focus();
  for (const [key, place] of [
    ["ArrowDown", 2],
    ["ArrowDown", 3],
    ["ArrowUp", 2],
  ] as const) {
    fireEvent.keyDown(document.activeElement!, { key, altKey: true });
    await waitFor(() => expect(titles()[place]).toBe("Track 1"));
    expect(document.activeElement).toBe(grip("Track 1"));
  }
});

test("dragging the grip two rows down lifts the row, steps neighbours aside and drops it there", async () => {
  await listed(5);
  await waitFor(() => expect(rows()).toHaveLength(5));
  const handle = grip("Track 0");
  pointer("pointerdown", handle, 20);
  await waitFor(() => expect(rows()[0].hasAttribute("data-lifted")).toBe(true));
  pointer("pointermove", handle, 20 + 2 * QUEUE_ROW);
  await waitFor(() =>
    expect(rows()[2].getAttribute("style")).toContain(`${QUEUE_ROW}px`),
  );
  pointer("pointerup", handle, 20 + 2 * QUEUE_ROW);
  await waitFor(() =>
    expect(titles()).toEqual([
      "Track 1",
      "Track 2",
      "Track 0",
      "Track 3",
      "Track 4",
    ]),
  );
  await waitFor(() =>
    expect(document.querySelector("[data-lifted]")).toBeNull(),
  );
});

test("Escape cancels a drag and nothing moves", async () => {
  await listed(4);
  await waitFor(() => expect(rows()).toHaveLength(4));
  const version = player.queueSnapshot().version;
  const handle = grip("Track 1");
  pointer("pointerdown", handle, 20);
  pointer("pointermove", handle, 20 + 2 * QUEUE_ROW);
  fireEvent.keyDown(window, { key: "Escape" });
  await waitFor(() =>
    expect(document.querySelector("[data-lifted]")).toBeNull(),
  );
  pointer("pointerup", handle, 20 + 2 * QUEUE_ROW);
  await Promise.resolve();
  expect(player.queueSnapshot().version).toBe(version);
  expect(titles()).toEqual(["Track 0", "Track 1", "Track 2", "Track 3"]);
});

test("releasing on the starting slot changes nothing", async () => {
  await listed(3);
  await waitFor(() => expect(rows()).toHaveLength(3));
  const version = player.queueSnapshot().version;
  const handle = grip("Track 1");
  pointer("pointerdown", handle, 20);
  pointer("pointermove", handle, 26);
  pointer("pointerup", handle, 26);
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
