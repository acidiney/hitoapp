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

test("a track that is not stored here says it plays from the services", () => {
  player.state.current = null;
  render(createElement(TrackAvailabilityNote, { trackId: "t1" }));
  expect(screen.getByText("Plays from your services")).toBeTruthy();
  expect(screen.queryByText("Unavailable on this device")).toBeNull();
});

test("the warning disappears while the track is the current one", () => {
  player.state.current = null;
  const { container } = render(
    createElement(TrackAvailabilityNote, { trackId: "t2" }),
  );
  act(() => {
    player.state.current = { id: "t2" };
    player.emit();
  });
  expect(container.textContent).toBe("");
});
