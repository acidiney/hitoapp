// @vitest-environment jsdom
import { createElement } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { Playback } from "../playback";
import PlayerDock from "./PlayerDock";

let player: Playback;
vi.mock("./PlaybackProvider", () => ({ usePlayer: () => player }));
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

test("position updates move the seek control without rendering a thousand queued rows", async () => {
  const audio = document.createElement("audio");
  vi.spyOn(audio, "load").mockImplementation(() => {
    audio.playbackRate = audio.defaultPlaybackRate;
  });
  vi.spyOn(audio, "pause").mockImplementation(() => {
    audio.dispatchEvent(new Event("pause"));
  });
  vi.spyOn(audio, "play").mockImplementation(async () => {
    audio.dispatchEvent(new Event("play"));
  });
  player = new Playback(audio, async (id) => [{ id, ready: true }]);
  let queuedRowReads = 0;
  const tracks = Array.from({ length: 1000 }, (_, index) => ({
    id: `track-${index}`,
    get title() {
      if (index > 0) queuedRowReads++;
      return `Track ${index}`;
    },
    artist: "Fixture artist",
    duration_ms: 10000,
    available: true,
  }));
  await player.playQueue(tracks);
  render(createElement(PlayerDock));
  fireEvent.click(screen.getByRole("button", { name: "Open queue" }));
  const queue = screen.getByRole("complementary", { name: "Playback queue" });
  await waitFor(() => expect(queue.querySelectorAll("li")).toHaveLength(50));
  fireEvent.click(screen.getByRole("button", { name: "Next queue page" }));
  await waitFor(() =>
    expect(queue.querySelector("li strong")?.textContent).toBe("Track 50"),
  );
  fireEvent.click(screen.getByRole("button", { name: "Previous queue page" }));
  await waitFor(() =>
    expect(queue.querySelector("li strong")?.textContent).toBe("Track 0"),
  );
  queuedRowReads = 0;
  for (let tick = 1; tick <= 10; tick++) {
    act(() => {
      audio.currentTime = tick / 10;
      audio.dispatchEvent(new Event("timeupdate"));
    });
  }
  expect(queuedRowReads).toBe(0);
  expect(screen.getByRole("slider", { name: "Seek" })).toHaveProperty(
    "value",
    "1",
  );
  fireEvent.change(screen.getByRole("combobox", { name: "Playback speed" }), {
    target: { value: "1.5" },
  });
  await act(async () => {
    await player.next();
  });
  expect(queue.querySelector("li.current strong")?.textContent).toBe("Track 1");
  expect(queuedRowReads).toBeGreaterThan(0);
  expect(audio.playbackRate).toBe(1.5);
  expect(
    screen.getByRole("combobox", { name: "Playback speed" }),
  ).toHaveProperty("value", "1.5");
});
