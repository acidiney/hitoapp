// @vitest-environment jsdom
import { expect, test, vi } from "vitest";
import { Playback } from "../playback";

test("queue pages retain duplicate occurrence IDs through selection and removal", async () => {
  const audio = document.createElement("audio");
  vi.spyOn(audio, "load").mockImplementation(() => {});
  vi.spyOn(audio, "pause").mockImplementation(() => {});
  vi.spyOn(audio, "play").mockResolvedValue();
  const player = new Playback(audio, async () => [
    { id: "source", ready: true },
  ]);
  const track = {
    id: "same",
    title: "Fixture",
    artist: "Test",
    duration_ms: 10000,
    available: true,
  };
  await player.playQueue([track, track]);
  const first = await player.queuePage(0, 50);
  expect(first.entries).toHaveLength(2);
  expect(first.entries[0].id).not.toBe(first.entries[1].id);
  await player.selectEntry(first.entries[1].id);
  expect(player.queueSnapshot().currentEntry).toBe(first.entries[1].id);
  player.removeEntry(first.entries[0].id);
  const second = await player.queuePage(0, 50);
  expect(second.entries.map((entry) => entry.id)).toEqual([
    first.entries[1].id,
  ]);
  expect(player.queueSnapshot().currentIndex).toBe(0);
  expect(player.queueSnapshot().currentEntry).toBe(first.entries[1].id);
});
