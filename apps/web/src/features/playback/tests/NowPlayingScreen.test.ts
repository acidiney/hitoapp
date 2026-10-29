import { createElement } from "react";
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
import { router } from "@inertiajs/react";
import { Playback } from "../playback";
import NowPlaying from "../components/NowPlaying";
import { QueueMedia, queueTrack } from "../fixtures/browserQueueMedia";

let player: Playback;
let media: QueueMedia;
const close = vi.fn();
const openQueue = vi.fn();
vi.mock("@inertiajs/react", () => ({ router: { visit: vi.fn() } }));
vi.mock("../components/PlaybackProvider", () => ({ usePlayer: () => player }));
vi.mock("../../artwork/components/Artwork", () => ({
  default: () => createElement("div", { "data-testid": "artwork" }),
}));

beforeEach(async () => {
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
  media = new QueueMedia();
  player = new Playback(media, async (id) => [{ id, ready: true }]);
  await player.playQueue(
    Array.from({ length: 8 }, (_, index) =>
      queueTrack(`r${index}`, `Track ${index}`),
    ),
  );
  render(createElement(NowPlaying, { close, openQueue }));
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  close.mockClear();
  openQueue.mockClear();
});

test("the full-screen player shows the track, transport and a tablist", () => {
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  expect(
    within(dialog).getByRole("heading", { name: "Track 0", level: 1 }),
  ).toBeTruthy();
  expect(within(dialog).getByRole("slider", { name: "Seek" })).toBeTruthy();
  for (const name of ["Shuffle", "Previous track", "Pause", "Next track"])
    expect(within(dialog).getByRole("button", { name })).toBeTruthy();
  const tabs = within(dialog).getAllByRole("tab");
  expect(tabs.map((tab) => tab.textContent)).toEqual([
    "Up next",
    "Lyrics",
    "Source",
  ]);
  expect(tabs[0].getAttribute("aria-selected")).toBe("true");
  expect(within(dialog).getByRole("tabpanel")).toBeTruthy();
});

test("Up next lists the next four queued tracks and opens the full queue", async () => {
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  await waitFor(() => expect(within(dialog).getByText("Track 4")).toBeTruthy());
  expect(within(dialog).queryByText("Track 5")).toBeNull();
  fireEvent.click(within(dialog).getByRole("link", { name: "See full queue" }));
  expect(openQueue).toHaveBeenCalledTimes(1);
});

test("arrow keys move between the tabs", () => {
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  const [next, lyrics] = within(dialog).getAllByRole("tab");
  next.focus();
  fireEvent.keyDown(next, { key: "ArrowRight" });
  expect(lyrics.getAttribute("aria-selected")).toBe("true");
  expect(document.activeElement).toBe(lyrics);
});

test("Lyrics says where lyrics can be found instead of inventing lines", () => {
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  fireEvent.click(within(dialog).getByRole("tab", { name: "Lyrics" }));
  const panel = within(dialog).getByRole("tabpanel");
  expect(panel.textContent).toMatch(/track details/i);
  expect(
    within(panel).getByRole("link", { name: "Track details" }),
  ).toHaveProperty("pathname", "/recordings/r0");
});

test("Source shows the speed the source plays at and sets the speed", () => {
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  fireEvent.click(within(dialog).getByRole("tab", { name: "Source" }));
  const speed = within(dialog).getByRole("radiogroup", { name: "Speed" });
  expect(within(speed).getByRole("radio", { name: "1×" })).toHaveProperty(
    "ariaChecked",
    "true",
  );
  vi.useFakeTimers();
  fireEvent.click(within(speed).getByRole("radio", { name: "1.5×" }));
  act(() => vi.advanceTimersByTime(400));
  vi.useRealTimers();
  expect(media.playbackRate).toBe(1.5);
});

test("rapid speed choices coalesce into one command", () => {
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  fireEvent.click(within(dialog).getByRole("tab", { name: "Source" }));
  const speed = within(dialog).getByRole("radiogroup", { name: "Speed" });
  const command = vi.spyOn(player, "speed");
  vi.useFakeTimers();
  for (const name of ["1.25×", "1.5×", "2×"])
    fireEvent.click(within(speed).getByRole("radio", { name }));
  expect(command).not.toHaveBeenCalled();
  expect(
    within(speed)
      .getByRole("radio", { name: "2×" })
      .getAttribute("aria-checked"),
  ).toBe("true");
  act(() => vi.advanceTimersByTime(400));
  vi.useRealTimers();
  expect(command).toHaveBeenCalledTimes(1);
  expect(command).toHaveBeenCalledWith(2);
});

test("following a link inside the player closes it and then focuses the new page", () => {
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  fireEvent.click(
    within(dialog).getByRole("link", { name: "Owned performer" }),
  );
  expect(close).toHaveBeenCalledTimes(1);
  fireEvent.click(within(dialog).getByRole("tab", { name: "Lyrics" }));
  fireEvent.click(within(dialog).getByRole("link", { name: "Track details" }));
  expect(close).toHaveBeenCalledTimes(2);
  const visit = vi.mocked(router.visit);
  const options = visit.mock.calls.at(-1)?.[1] as { onSuccess(): void };
  document.body.insertAdjacentHTML(
    "beforeend",
    '<main><h1 id="landing">Details</h1></main>',
  );
  vi.useFakeTimers();
  options.onSuccess();
  vi.runAllTimers();
  vi.useRealTimers();
  expect(document.activeElement?.id).toBe("landing");
});

test("position ticks do not fetch queue pages", async () => {
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  await waitFor(() => expect(within(dialog).getByText("Track 4")).toBeTruthy());
  const pages = vi.spyOn(player, "queuePage");
  for (let tick = 1; tick <= 5; tick++)
    act(() => {
      media.currentTime = tick;
      media.dispatchEvent(new Event("timeupdate"));
    });
  expect(pages).not.toHaveBeenCalled();
  expect(within(dialog).getByRole("slider", { name: "Seek" })).toHaveProperty(
    "value",
    "5",
  );
});

test("the close button closes the dialog", () => {
  fireEvent.click(screen.getByRole("button", { name: "Close now playing" }));
  expect(close).toHaveBeenCalledTimes(1);
});
