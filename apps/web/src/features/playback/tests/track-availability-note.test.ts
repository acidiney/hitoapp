import { createElement } from "react";
import { act, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import TrackAvailabilityNote from "../components/TrackAvailabilityNote";

const player = vi.hoisted(() => {
  const listeners = new Set<() => void>();
  const state = { current: null as { id: string } | null };
  return {
    state,
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    snapshot: () => state,
    emit() {
      listeners.forEach((listener) => listener());
    },
  };
});
vi.mock("../components/PlaybackProvider", () => ({
  usePlayer: () => player,
}));

const remote = (id: string, plays_from_services: boolean) => ({
  id,
  available: false,
  plays_from_services,
});

test("a track not stored here that a service can resolve says it plays from the services", () => {
  player.state.current = null;
  render(createElement(TrackAvailabilityNote, { track: remote("t1", true) }));
  expect(screen.getByText("Plays from your services")).toBeTruthy();
  expect(screen.queryByText("Not playable yet")).toBeNull();
});

test("a track not stored here that no service can resolve is muted as not playable yet", () => {
  player.state.current = null;
  const { container } = render(
    createElement(TrackAvailabilityNote, { track: remote("t3", false) }),
  );
  expect(screen.getByText("Not playable yet")).toBeTruthy();
  expect(screen.queryByText("Plays from your services")).toBeNull();
  expect(
    container.querySelector(".availability")?.getAttribute("data-state"),
  ).toBe("catalog");
});

test("a local track shows no note whatever the services say", () => {
  player.state.current = null;
  const { container } = render(
    createElement(TrackAvailabilityNote, {
      track: { id: "t4", available: true, plays_from_services: true },
    }),
  );
  expect(container.textContent).toBe("");
});

test("a track without the server answer claims nothing", () => {
  player.state.current = null;
  const { container } = render(
    createElement(TrackAvailabilityNote, {
      track: { id: "t5", available: false },
    }),
  );
  expect(container.textContent).toBe("");
});

test("the services note disappears while the track is the current one", () => {
  player.state.current = null;
  const { container } = render(
    createElement(TrackAvailabilityNote, { track: remote("t2", true) }),
  );
  act(() => {
    player.state.current = { id: "t2" };
    player.emit();
  });
  expect(container.textContent).toBe("");
});
