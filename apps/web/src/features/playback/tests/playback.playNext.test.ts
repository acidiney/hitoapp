import { afterEach, expect, test, vi } from "vitest";
import { Playback } from "../playback";

afterEach(() => vi.restoreAllMocks());

function setup() {
  const audio = document.createElement("audio");
  vi.spyOn(audio, "load").mockImplementation(() => {});
  vi.spyOn(audio, "pause").mockImplementation(() => {});
  vi.spyOn(audio, "play").mockResolvedValue();
  return new Playback(audio, async () => [{ id: "source", ready: true }]);
}

const track = (id: string) => ({
  id,
  title: id,
  artist: "Test",
  duration_ms: 10000,
  available: true,
});

test("play next places the track right after the current entry", async () => {
  const player = setup();
  await player.playQueue([track("a"), track("b"), track("c")], 0);
  await player.playNext(track("x"));
  const page = await player.queuePage(0, 10);
  expect(page.entries.map((entry) => entry.recording.id)).toEqual([
    "a",
    "x",
    "b",
    "c",
  ]);
  expect(player.queueSnapshot().currentIndex).toBe(0);
});

test("play next at the end of the queue appends, and on an empty queue starts playing", async () => {
  const player = setup();
  await player.playNext(track("solo"));
  expect(player.snapshot().current?.id).toBe("solo");
  await player.playQueue([track("a"), track("b")], 1);
  await player.playNext(track("x"));
  const page = await player.queuePage(0, 10);
  expect(page.entries.map((entry) => entry.recording.id)).toEqual([
    "a",
    "b",
    "x",
  ]);
  expect(player.queueSnapshot().currentIndex).toBe(1);
});
