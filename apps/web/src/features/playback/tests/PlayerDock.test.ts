import { createElement } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { Playback } from "../playback";
import PlayerDock from "../components/PlayerDock";
import { QUEUE_ROW } from "../hooks/useQueueWindow";
import QueueScreen from "../components/QueueScreen";

let player: Playback;
vi.mock("../components/PlaybackProvider", () => ({ usePlayer: () => player }));
test("position updates move the seek control without rendering any of a thousand queued rows", async () => {
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
  const queuePage = player.queuePage.bind(player);
  vi.spyOn(player, "queuePage").mockImplementation(async (...args) => {
    const page = await queuePage(...args);
    return {
      ...page,
      entries: page.entries.map((entry) => ({
        ...entry,
        recording: {
          ...entry.recording,
          get title() {
            queuedRowReads++;
            return entry.recording.title;
          },
        },
      })),
    };
  });
  const tracks = Array.from({ length: 1000 }, (_, index) => ({
    id: `track-${index}`,
    title: `Track ${index}`,
    artist: "Fixture artist",
    duration_ms: 10000,
    available: true,
  }));
  await player.playQueue(tracks);
  render(createElement(PlayerDock, { account: "alice" }));
  render(createElement(QueueScreen));
  const queue = screen.getByRole("region", { name: "Up next" });
  await waitFor(() =>
    expect(queue.querySelectorAll("li").length).toBeGreaterThan(0),
  );
  expect(queue.querySelectorAll("li").length).toBeLessThanOrEqual(40);
  const window = queue.querySelector(".queue-window")!;
  Object.defineProperty(window, "scrollTop", {
    value: 500 * QUEUE_ROW,
    configurable: true,
  });
  fireEvent.scroll(window);
  await waitFor(() => expect(queue.textContent).toContain("Track 501"));
  expect(queue.querySelectorAll("li").length).toBeLessThanOrEqual(40);
  Object.defineProperty(window, "scrollTop", { value: 0, configurable: true });
  fireEvent.scroll(window);
  await waitFor(() =>
    expect(queue.querySelector("li strong")?.textContent).toBe("Track 1"),
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
  queuedRowReads = 0;
  await act(async () => {
    await player.next();
  });
  await waitFor(() =>
    expect(queue.querySelector("li strong")?.textContent).toBe("Track 2"),
  );
  expect(
    screen.getByRole("region", { name: "Now playing" }).textContent,
  ).toContain("Track 1");
  expect(queuedRowReads).toBeGreaterThan(0);
  expect(audio.playbackRate).toBe(1.5);
  expect(
    screen.getByRole("combobox", { name: "Playback speed" }),
  ).toHaveProperty("value", "1.5");
});
