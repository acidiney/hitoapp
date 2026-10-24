import { afterEach, expect, test, vi } from "vitest";
import { Playback } from "../playback";
import type { SourceResolver } from "../api/playbackSources";
import {
  QueueMedia,
  queueMediaOwner,
  queueTrack,
} from "../fixtures/browserQueueMedia";

afterEach(() => vi.restoreAllMocks());

async function started(count: number, index = 0) {
  const media = new QueueMedia();
  const player = new Playback(media, async (id) => [{ id, ready: true }]);
  await player.playQueue(
    Array.from({ length: count }, (_, i) => queueTrack(`r${i}`, `Track ${i}`)),
    index,
  );
  const ids = async () =>
    (await player.queuePage(0, 1000)).entries.map((entry) => entry.id);
  const titles = async () =>
    (await player.queuePage(0, 1000)).entries.map(
      (entry) => entry.recording.title,
    );
  return { media, player, ids, titles };
}

test("moving an entry keeps the playing occurrence, its index mapping and its audio", async () => {
  const { media, player, ids, titles } = await started(5, 2);
  const [, , playing] = await ids();
  const loads = media.loads.length;
  const plays = media.played.length;
  const before = player.queueSnapshot();

  player.moveEntry((await ids())[4], (await ids())[0]);

  expect(await titles()).toEqual([
    "Track 4",
    "Track 0",
    "Track 1",
    "Track 2",
    "Track 3",
  ]);
  const after = player.queueSnapshot();
  expect(after.currentEntry).toBe(playing);
  expect(after.currentIndex).toBe(3);
  expect(after.total).toBe(5);
  expect(after.version).toBeGreaterThan(before.version);
  expect(player.snapshot().current?.title).toBe("Track 2");
  expect(media.loads).toHaveLength(loads);
  expect(media.played).toHaveLength(plays);
  expect(media.paused).toBe(false);
});

test("moving the playing entry keeps its identity and does not restart it", async () => {
  const { media, player, ids, titles } = await started(4, 1);
  const [, playing, , last] = await ids();
  const loads = media.loads.length;

  player.moveEntry(playing, null);
  expect(await titles()).toEqual(["Track 0", "Track 2", "Track 3", "Track 1"]);
  expect(player.queueSnapshot().currentEntry).toBe(playing);
  expect(player.queueSnapshot().currentIndex).toBe(3);
  expect(media.loads).toHaveLength(loads);

  player.moveEntry(playing, last);
  expect(await titles()).toEqual(["Track 0", "Track 2", "Track 1", "Track 3"]);
  expect(player.queueSnapshot().currentIndex).toBe(2);
  expect(media.loads).toHaveLength(loads);
});

test("a move to the same place or of an unknown entry changes nothing", async () => {
  const { player, ids, titles } = await started(3);
  const [a, b, c] = await ids();
  const version = player.queueSnapshot().version;
  player.moveEntry(b, b);
  player.moveEntry(b, c);
  player.moveEntry(c, null);
  player.moveEntry("missing", a);
  player.moveEntry(a, "missing");
  expect(await titles()).toEqual(["Track 0", "Track 1", "Track 2"]);
  expect(player.queueSnapshot().version).toBe(version);
});

test("natural advance follows the moved order", async () => {
  const { media, player, ids, titles } = await started(4);
  const [, , c] = await ids();
  player.moveEntry(c, (await ids())[1]);
  expect(await titles()).toEqual(["Track 0", "Track 2", "Track 1", "Track 3"]);
  media.ended = true;
  media.dispatchEvent(new Event("ended"));
  await vi.waitFor(() => expect(player.queueSnapshot().currentEntry).toBe(c));
  expect(player.snapshot().current?.title).toBe("Track 2");
});

test("shuffled advance still visits every occurrence once after a move", async () => {
  const { player, ids } = await started(6);
  player.shuffle();
  const all = await ids();
  player.moveEntry(all[5], all[1]);
  const seen = new Set([player.queueSnapshot().currentEntry]);
  for (let step = 1; step < 6; step++) {
    await player.next();
    seen.add(player.queueSnapshot().currentEntry);
  }
  expect(seen.size).toBe(6);
});

test("moving a different entry into the next slot cancels stale preparation; unrelated moves keep it", async () => {
  const initial = new QueueMedia();
  const owner = queueMediaOwner(initial);
  const requests: { recording: string; signal: AbortSignal }[] = [];
  const resolve: SourceResolver = async (recording, options) => {
    requests.push({ recording, signal: options.signal });
    if (recording === "r0") return [{ id: "audio-r0", ready: true }];
    return new Promise(() => {});
  };
  const player = new Playback(initial, resolve, undefined, owner);
  await player.playQueue(["r0", "r1", "r2", "r3"].map((id) => queueTrack(id)));
  await vi.waitFor(() =>
    expect(requests.map((r) => r.recording)).toContain("r1"),
  );
  const next = requests.find((r) => r.recording === "r1")!;
  const [, , c, d] = (await player.queuePage(0, 10)).entries;

  player.moveEntry(d.id, c.id);
  expect(next.signal.aborted).toBe(false);
  expect(requests.filter((r) => r.recording === "r1")).toHaveLength(1);

  player.moveEntry(c.id, (await player.queuePage(1, 1)).entries[0].id);
  expect(next.signal.aborted).toBe(true);
  await vi.waitFor(() =>
    expect(
      requests.some((r) => r.recording === "r2" && !r.signal.aborted),
    ).toBe(true),
  );
  expect(initial.paused).toBe(false);
});
