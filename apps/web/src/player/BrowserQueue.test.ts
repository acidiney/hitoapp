// @vitest-environment jsdom
import { afterEach, expect, test, vi } from "vitest";
import { Playback } from "../playback";

afterEach(() => vi.restoreAllMocks());

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

test("browser shuffle visits each occurrence once and stops with repeat off", async () => {
  vi.spyOn(Math, "random").mockReturnValue(0.5);
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
  await player.playQueue(Array(4).fill(track), 3);
  const canonical = (await player.queuePage(0, 50)).entries.map(
    (entry) => entry.id,
  );
  player.shuffle();
  const visited = new Set([player.queueSnapshot().currentEntry]);
  for (let i = 1; i < 4; i++) {
    await player.next();
    const next = player.queueSnapshot().currentEntry;
    expect(next).toBeTruthy();
    expect(visited.has(next)).toBe(false);
    visited.add(next);
  }
  expect(visited).toEqual(new Set(canonical));
  await player.next();
  expect(player.snapshot().current).toBeNull();
  expect(
    (await player.queuePage(0, 50)).entries.map((entry) => entry.id),
  ).toEqual(canonical);
});
