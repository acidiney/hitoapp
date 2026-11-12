import { createElement } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import FollowedLyrics from "../components/FollowedLyrics";

const state = vi.hoisted(() => ({
  current: null as string | null,
  position: 0,
  positionListeners: new Set<() => void>(),
  listeners: new Set<() => void>(),
  seek: vi.fn(),
}));

vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => ({
    subscribe: (listener: () => void) => {
      state.listeners.add(listener);
      return () => state.listeners.delete(listener);
    },
    snapshot: () => ({ current: state.current ? { id: state.current } : null }),
    positionSnapshot: () => ({ position: state.position, duration: 200 }),
    subscribePosition: (listener: () => void) => {
      state.positionListeners.add(listener);
      return () => state.positionListeners.delete(listener);
    },
    seek: state.seek,
  }),
}));

const lines = [
  { start_ms: 1000, end_ms: 4000, text: "First" },
  { start_ms: 5000, end_ms: 9000, text: "Second" },
  { start_ms: 10000, end_ms: 12000, text: "Third" },
];

beforeEach(() => {
  state.current = null;
  state.position = 0;
  Element.prototype.scrollBy = vi.fn();
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

function setPosition(seconds: number) {
  act(() => {
    state.position = seconds;
    state.positionListeners.forEach((listener) => listener());
  });
}

test("a recording that is not playing shows every line as upcoming, with no current line, no dimming of past and no seeking", () => {
  state.current = "another-recording";
  state.position = 11;
  render(createElement(FollowedLyrics, { recording: "mine", lines }));
  const buttons = screen.getAllByRole("button");
  expect(buttons.map((button) => button.getAttribute("data-state"))).toEqual([
    "next",
    "next",
    "next",
  ]);
  expect(document.querySelector("[aria-current]")).toBeNull();
  fireEvent.click(buttons[1]);
  expect(state.seek).not.toHaveBeenCalled();
  setPosition(6);
  expect(document.querySelector("[aria-current]")).toBeNull();
});

test("when this recording is the one playing the current line follows the position and a click seeks it", () => {
  state.current = "mine";
  render(createElement(FollowedLyrics, { recording: "mine", lines }));
  setPosition(5.5);
  const second = screen.getByRole("button", { name: "Second" });
  expect(second.getAttribute("aria-current")).toBe("true");
  expect(
    screen.getByRole("button", { name: "First" }).getAttribute("data-state"),
  ).toBe("past");
  expect(
    screen.getByRole("button", { name: "Third" }).getAttribute("data-state"),
  ).toBe("next");
  fireEvent.click(second);
  expect(state.seek).toHaveBeenCalledWith(5);
});

test("following scrolls the lines container itself and never the page", () => {
  state.current = "mine";
  const scrollIntoView = vi.fn();
  Element.prototype.scrollIntoView = scrollIntoView;
  render(createElement(FollowedLyrics, { recording: "mine", lines }));
  setPosition(10.5);
  expect(Element.prototype.scrollBy).toHaveBeenCalled();
  expect(scrollIntoView).not.toHaveBeenCalled();
});
