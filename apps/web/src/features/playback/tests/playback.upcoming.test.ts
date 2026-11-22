import { expect, test } from "vitest";
import { Playback } from "../playback";
import type { Upcoming } from "../browser/BrowserUpcoming";
import { QueueMedia, queueTrack } from "../fixtures/browserQueueMedia";

function setup() {
  const media = new QueueMedia();
  const announced: Upcoming[] = [];
  const player = new Playback(
    media,
    async () => [{ id: "source", ready: true }],
    undefined,
    undefined,
    undefined,
    undefined,
    undefined,
    undefined,
    (upcoming) => announced.push(upcoming),
  );
  const last = () => announced.at(-1);
  return { player, media, announced, last };
}

const tracks = ["a", "b", "c", "d"].map((id) => queueTrack(id));

test("starting a track names it and the next two in play order", async () => {
  const { player, last } = setup();
  await player.playQueue(tracks, 1);
  expect(last()).toEqual({ current: "b", next: ["c", "d"] });
  await player.next();
  expect(last()).toEqual({ current: "c", next: ["d"] });
  await player.next();
  expect(last()).toEqual({ current: "d", next: [] });
});

test("queue edits, repeat and shuffle announce the new next tracks", async () => {
  const { player, last } = setup();
  await player.playQueue(tracks, 0);
  const page = await player.queuePage(0, 4);
  player.moveEntry(page.entries[3].id, page.entries[1].id);
  expect(last()).toEqual({ current: "a", next: ["d", "b"] });
  player.removeEntry(page.entries[3].id);
  expect(last()).toEqual({ current: "a", next: ["b", "c"] });
  await player.addToQueue(queueTrack("e"));
  await player.playNext(queueTrack("f"));
  expect(last()).toEqual({ current: "a", next: ["f", "b"] });
  player.repeat();
  player.repeat();
  expect(player.snapshot().repeat).toBe("one");
  expect(last()).toEqual({ current: "a", next: [] });
  player.repeat();
  player.shuffle();
  const shuffled = last()!;
  expect(shuffled.current).toBe("a");
  expect(shuffled.next).toHaveLength(2);
  expect(new Set(shuffled.next).size).toBe(2);
  expect(shuffled.next).not.toContain("a");
});

test("repeat all wraps the next tracks around the end of the queue", async () => {
  const { player, last } = setup();
  await player.playQueue(tracks, 3);
  expect(last()).toEqual({ current: "d", next: [] });
  player.repeat();
  expect(player.snapshot().repeat).toBe("all");
  expect(last()).toEqual({ current: "d", next: ["a", "b"] });
});

test("clearing the queue and reaching its end announce that nothing plays", async () => {
  const { player, last } = setup();
  await player.playQueue(tracks.slice(0, 1), 0);
  expect(last()).toEqual({ current: "a", next: [] });
  await player.next();
  expect(last()).toEqual({ current: null, next: [] });
  await player.playQueue(tracks, 0);
  player.clear();
  expect(last()).toEqual({ current: null, next: [] });
});

test("a restored queue that has not started announces nothing playing", async () => {
  const { player, announced } = setup();
  expect(player.restoreQueue(tracks, 2, 12)).toBe(true);
  expect(announced.every((item) => item.current === null)).toBe(true);
  await player.setPlaying(true);
  expect(announced.at(-1)).toEqual({ current: "c", next: ["d"] });
});

test("a failing announcement callback never stops playback", async () => {
  const media = new QueueMedia();
  const player = new Playback(
    media,
    async () => [{ id: "source", ready: true }],
    undefined,
    undefined,
    undefined,
    undefined,
    undefined,
    undefined,
    () => {
      throw new Error("announcement broke");
    },
  );
  await player.playQueue(tracks, 0);
  expect(player.snapshot()).toMatchObject({ playing: true, error: "" });
  expect(player.snapshot().current?.id).toBe("a");
});

test("audiobook chapters are never announced", async () => {
  const { player, last } = setup();
  const chapter = (id: string) => ({
    recording: queueTrack(id),
    chapter: {
      audiobook_id: "book",
      entry_id: `entry-${id}`,
      recording_id: id,
    },
  });
  await player.playQueue([chapter("one"), chapter("two")], 0);
  expect(last()).toEqual({ current: null, next: [] });
  await player.playQueue([queueTrack("a"), chapter("two"), queueTrack("b")], 0);
  expect(last()).toEqual({ current: "a", next: ["b"] });
});
