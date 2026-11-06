import { expect, test } from "vitest";
import { Playback } from "../playback";
import { QueueMedia, queueTrack } from "../fixtures/browserQueueMedia";

const tracks = [
  queueTrack("a", "A"),
  queueTrack("b", "B"),
  queueTrack("c", "C"),
];

function restored(position = 12) {
  const media = new QueueMedia();
  const player = new Playback(media, async (id) => [{ id, ready: true }]);
  expect(player.restoreQueue(tracks, 1, position)).toBe(true);
  return { media, player };
}

test("a restored queue shows the track, the position and the queue without touching audio", () => {
  const { media, player } = restored();
  expect(player.snapshot().current?.title).toBe("B");
  expect(player.snapshot().playing).toBe(false);
  expect(player.snapshot().loading).toBe(false);
  expect(player.positionSnapshot()).toEqual({ position: 12, duration: 90 });
  expect(player.queueSnapshot().total).toBe(3);
  expect(player.queueSnapshot().currentIndex).toBe(1);
  expect(media.loads).toEqual([]);
  expect(media.played).toEqual([]);
});

test("pressing play opens the restored track at the restored position", async () => {
  const { media, player } = restored();
  await player.setPlaying(true);
  expect(media.played).toHaveLength(1);
  expect(player.snapshot().current?.title).toBe("B");
  expect(player.positionSnapshot().position).toBe(12);
});

test("seeking before play keeps the new position for the first play", async () => {
  const { media, player } = restored();
  player.seek(30);
  expect(media.loads).toEqual([]);
  expect(player.positionSnapshot().position).toBe(30);
  await player.toggle();
  expect(media.played).toHaveLength(1);
  expect(player.positionSnapshot().position).toBe(30);
});

test("next and previous open a neighbour instead of resuming", async () => {
  const { media, player } = restored();
  await player.next();
  expect(player.snapshot().current?.title).toBe("C");
  expect(media.played).toHaveLength(1);
});

test("pausing a restored queue does nothing and a live queue is never replaced", async () => {
  const { media, player } = restored();
  await player.setPlaying(false);
  expect(media.played).toEqual([]);
  expect(player.restoreQueue([queueTrack("z", "Z")], 0, 0)).toBe(false);
  expect(player.snapshot().current?.title).toBe("B");
});

test("a restore at a missing index is refused", () => {
  const player = new Playback(new QueueMedia(), async () => []);
  expect(player.restoreQueue(tracks, 7, 0)).toBe(false);
  expect(player.queueSnapshot().total).toBe(0);
});

test("liking a track updates the queued copy and the current track", async () => {
  const { player } = restored();
  player.setRecordingLiked("b", true);
  expect(player.snapshot().current?.liked).toBe(true);
  const page = await player.queuePage(0, 3);
  expect(page.entries.map((entry) => entry.recording.liked)).toEqual([
    undefined,
    true,
    undefined,
  ]);
});
