import { createElement } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import PlayShuffle from "../components/PlayShuffle";
import type { Track } from "../model";

const player = vi.hoisted(() => ({
  playQueue: vi.fn(async () => {}),
  shuffle: vi.fn(),
  snapshot: vi.fn(() => ({ shuffle: false })),
}));
vi.mock("../components/PlaybackProvider", () => ({ usePlayer: () => player }));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const tracks = [
  { id: "a", title: "A", artist: "X", duration_ms: 1000, available: true },
  { id: "b", title: "B", artist: "X", duration_ms: 1000, available: true },
] as unknown as Track[];

test("play starts the tracks in order", async () => {
  render(createElement(PlayShuffle, { tracks, label: "album" }));
  fireEvent.click(screen.getByRole("button", { name: "Play album" }));
  await vi.waitFor(() => expect(player.playQueue).toHaveBeenCalledTimes(1));
  expect(player.shuffle).not.toHaveBeenCalled();
});

test("shuffle plays the tracks and turns shuffle on once", async () => {
  render(createElement(PlayShuffle, { tracks, label: "album" }));
  fireEvent.click(screen.getByRole("button", { name: "Shuffle album" }));
  await vi.waitFor(() => expect(player.shuffle).toHaveBeenCalledTimes(1));
  expect(player.playQueue).toHaveBeenCalledTimes(1);
});

test("shuffle leaves shuffle on when it is already on", async () => {
  player.snapshot.mockReturnValue({ shuffle: true });
  render(createElement(PlayShuffle, { tracks, label: "album" }));
  fireEvent.click(screen.getByRole("button", { name: "Shuffle album" }));
  await vi.waitFor(() => expect(player.playQueue).toHaveBeenCalledTimes(1));
  expect(player.shuffle).not.toHaveBeenCalled();
});

test("both buttons are disabled when there is nothing to play", () => {
  render(createElement(PlayShuffle, { tracks: [], label: "playlist" }));
  expect(
    (screen.getByRole("button", { name: "Play playlist" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  expect(
    (
      screen.getByRole("button", {
        name: "Shuffle playlist",
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(true);
});

test("shuffle does not turn shuffle on when playback could not start", async () => {
  player.playQueue.mockRejectedValueOnce(new Error("no audio"));
  render(createElement(PlayShuffle, { tracks, label: "album" }));
  fireEvent.click(screen.getByRole("button", { name: "Shuffle album" }));
  await vi.waitFor(() => expect(player.playQueue).toHaveBeenCalledTimes(1));
  await Promise.resolve();
  expect(player.shuffle).not.toHaveBeenCalled();
});

test("shuffle starts from a random track, not always the first", async () => {
  const many = Array.from({ length: 50 }, (_, index) => ({
    ...tracks[0],
    id: `t${index}`,
  })) as unknown as Track[];
  vi.spyOn(Math, "random").mockReturnValue(0.5);
  render(createElement(PlayShuffle, { tracks: many, label: "album" }));
  fireEvent.click(screen.getByRole("button", { name: "Shuffle album" }));
  await vi.waitFor(() => expect(player.playQueue).toHaveBeenCalledTimes(1));
  const call = player.playQueue.mock.calls[0] as unknown as [Track[], number];
  expect(call[1]).toBe(25);
  vi.restoreAllMocks();
});
