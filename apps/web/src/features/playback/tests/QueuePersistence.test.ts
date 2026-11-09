import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { Playback } from "../playback";
import { QueuePersistence } from "../browser/QueuePersistence";
import {
  clearStoredQueues,
  parseStoredQueue,
  queueStorageKey,
} from "../browser/QueueStorage";
import { QueueMedia, queueTrack } from "../fixtures/browserQueueMedia";
import type { Track } from "../model";

const tracks = Array.from({ length: 4 }, (_, index) =>
  queueTrack(`r${index}`, `Track ${index}`),
);

function playback() {
  const media = new QueueMedia();
  const player = new Playback(media, async (id) => [{ id, ready: true }]);
  return { media, player };
}

function stored(account: string) {
  return localStorage.getItem(queueStorageKey(account));
}

function library(known: Track[] = tracks) {
  return vi.fn(async (ids: string[]) =>
    known.filter((track) => ids.includes(track.id)),
  );
}

async function settled() {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

beforeEach(() => {
  localStorage.clear();
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("a queue is written as ids under a versioned key for the signed-in account", async () => {
  const { player } = playback();
  const stop = new QueuePersistence(player, "alice").start();
  await player.playQueue(tracks, 2);
  await vi.waitFor(() => expect(stored("alice")).not.toBeNull());
  expect(queueStorageKey("alice")).toBe("hito:queue:v1:alice");
  const queue = parseStoredQueue(stored("alice"));
  expect(queue?.index).toBe(2);
  expect(queue?.entries.map((entry) => entry.id)).toEqual([
    "r0",
    "r1",
    "r2",
    "r3",
  ]);
  expect(stored("alice")).not.toContain("Track 2");
  expect(stored("alice")).not.toContain("Owned performer");
  expect(stored("bob")).toBeNull();
  stop();
});

test("a reload restores the queue paused at the same track and whole second", async () => {
  let now = 1_000_000;
  const first = playback();
  const stop = new QueuePersistence(first.player, "alice", {
    now: () => now,
  }).start();
  await first.player.playQueue(tracks, 1);
  await vi.waitFor(() => expect(stored("alice")).not.toBeNull());
  now += 10_000;
  first.player.seek(42.6);
  await vi.waitFor(() =>
    expect(parseStoredQueue(stored("alice"))?.position).toBe(43),
  );
  stop();

  const second = playback();
  const lookup = library();
  await new QueuePersistence(second.player, "alice", { lookup }).restore(
    new AbortController().signal,
  );

  expect(lookup).toHaveBeenCalledWith(
    ["r0", "r1", "r2", "r3"],
    expect.anything(),
  );
  const view = second.player.snapshot();
  expect(view.current?.title).toBe("Track 1");
  expect(view.playing).toBe(false);
  expect(view.loading).toBe(false);
  expect(second.player.positionSnapshot().position).toBe(43);
  expect(second.player.queueSnapshot().total).toBe(4);
  expect(second.player.queueSnapshot().currentIndex).toBe(1);
  expect(second.media.played).toEqual([]);
  expect(second.media.loads).toEqual([]);

  await second.player.toggle();
  expect(second.media.played).toHaveLength(1);
  expect(second.player.snapshot().current?.title).toBe("Track 1");
});

test("position writes are throttled while playing", async () => {
  let now = 1_000_000;
  const { player, media } = playback();
  const stop = new QueuePersistence(player, "alice", {
    now: () => now,
  }).start();
  await player.playQueue(tracks, 0);
  await vi.waitFor(() => expect(stored("alice")).not.toBeNull());
  await settled();
  const writes = vi.spyOn(Storage.prototype, "setItem");
  for (const seconds of [1, 2, 3]) {
    now += 1000;
    media.currentTime = seconds;
    media.dispatchEvent(new Event("timeupdate"));
  }
  await settled();
  expect(writes).not.toHaveBeenCalled();
  now += 6000;
  media.currentTime = 9;
  media.dispatchEvent(new Event("timeupdate"));
  await vi.waitFor(() => expect(writes).toHaveBeenCalledTimes(1));
  expect(parseStoredQueue(stored("alice"))?.position).toBe(9);
  stop();
});

test("another account neither restores nor keeps the previous account's queue", async () => {
  const { player } = playback();
  const stop = new QueuePersistence(player, "alice").start();
  await player.playQueue(tracks, 1);
  await vi.waitFor(() => expect(stored("alice")).not.toBeNull());
  stop();

  const other = playback();
  const lookup = library();
  await new QueuePersistence(other.player, "bob", { lookup }).restore(
    new AbortController().signal,
  );
  expect(lookup).not.toHaveBeenCalled();
  expect(other.player.snapshot().current).toBeNull();
  expect(other.player.queueSnapshot().total).toBe(0);
  expect(stored("alice")).toBeNull();
});

test("signing out clears every stored queue", async () => {
  const { player } = playback();
  const stop = new QueuePersistence(player, "alice").start();
  await player.playQueue(tracks, 0);
  await vi.waitFor(() => expect(stored("alice")).not.toBeNull());
  localStorage.setItem("hito:other", "kept");
  player.clear();
  clearStoredQueues();
  await settled();
  expect(stored("alice")).toBeNull();
  expect(localStorage.getItem("hito:other")).toBe("kept");
  stop();
});

test("clearing the queue forgets the stored copy", async () => {
  const { player } = playback();
  const stop = new QueuePersistence(player, "alice").start();
  await player.playQueue(tracks, 0);
  await vi.waitFor(() => expect(stored("alice")).not.toBeNull());
  player.clear();
  await vi.waitFor(() => expect(stored("alice")).toBeNull());
  stop();
});

test("corrupted, foreign and outdated storage is ignored and removed", async () => {
  const cases = [
    "{not json",
    "[]",
    JSON.stringify({ version: 99, index: 0, position: 0, entries: [] }),
    JSON.stringify({ version: 1, index: 5, position: 0, entries: [] }),
    JSON.stringify({
      version: 1,
      index: 0,
      position: 0,
      entries: [{ id: 7 }],
    }),
  ];
  for (const value of cases) {
    localStorage.setItem(queueStorageKey("alice"), value);
    const { player } = playback();
    const lookup = library();
    await new QueuePersistence(player, "alice", { lookup }).restore(
      new AbortController().signal,
    );
    expect(lookup).not.toHaveBeenCalled();
    expect(player.snapshot().current).toBeNull();
    expect(stored("alice")).toBeNull();
  }
});

test("storage that throws never breaks playback", async () => {
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new DOMException("full", "QuotaExceededError");
  });
  const { player } = playback();
  const persistence = new QueuePersistence(player, "alice");
  await persistence.restore(new AbortController().signal);
  const stop = persistence.start();
  await player.playQueue(tracks, 0);
  await settled();
  expect(player.snapshot().current?.title).toBe("Track 0");
  stop();
});

test("entries that no longer resolve are dropped on restore", async () => {
  const first = playback();
  const stop = new QueuePersistence(first.player, "alice").start();
  await first.player.playQueue(tracks, 1);
  await vi.waitFor(() => expect(stored("alice")).not.toBeNull());
  stop();

  const raw = JSON.parse(stored("alice")!);
  raw.entries.splice(3, 0, { id: "" });
  localStorage.setItem(queueStorageKey("alice"), JSON.stringify(raw));

  const second = playback();
  await new QueuePersistence(second.player, "alice", {
    lookup: library(tracks.filter((track) => track.id !== "r1")),
  }).restore(new AbortController().signal);
  expect(second.player.queueSnapshot().total).toBe(3);
  expect(second.player.snapshot().current?.title).toBe("Track 2");
  expect(second.player.positionSnapshot().position).toBe(0);
});

test("a lookup that cannot be made keeps the stored queue for the next load", async () => {
  const first = playback();
  const stop = new QueuePersistence(first.player, "alice").start();
  await first.player.playQueue(tracks, 1);
  await vi.waitFor(() => expect(stored("alice")).not.toBeNull());
  stop();
  const saved = stored("alice");

  const second = playback();
  const persistence = new QueuePersistence(second.player, "alice", {
    lookup: async () => {
      throw new Error("offline");
    },
  });
  await expect(
    persistence.restore(new AbortController().signal),
  ).rejects.toThrow("offline");
  const startedAgain = persistence.start();
  await settled();
  expect(stored("alice")).toBe(saved);
  startedAgain();
});

test("a queue started before the restore finishes is not replaced", async () => {
  const first = playback();
  const stop = new QueuePersistence(first.player, "alice").start();
  await first.player.playQueue(tracks, 1);
  await vi.waitFor(() => expect(stored("alice")).not.toBeNull());
  stop();

  const second = playback();
  let release: (found: Track[]) => void = () => {};
  const restoring = new QueuePersistence(second.player, "alice", {
    lookup: () => new Promise<Track[]>((resolve) => (release = resolve)),
  }).restore(new AbortController().signal);
  await second.player.playQueue([queueTrack("fresh", "Fresh")]);
  release(tracks);
  await restoring;
  expect(second.player.snapshot().current?.title).toBe("Fresh");
  expect(second.player.queueSnapshot().total).toBe(1);
});

test("a very long queue stores a bounded window of ids around the playing track", async () => {
  const { player } = playback();
  const stop = new QueuePersistence(player, "alice").start();
  const many = Array.from({ length: 2000 }, (_, index) =>
    queueTrack(`m${index}`, `Many ${index}`),
  );
  await player.playQueue(many, 1000);
  await vi.waitFor(() => expect(stored("alice")).not.toBeNull());
  const queue = parseStoredQueue(stored("alice"));
  expect(queue?.entries.length).toBe(500);
  expect(queue?.entries[queue.index].id).toBe("m1000");
  stop();
});

test("a restored queue keeps the credited artists the lookup returns", async () => {
  const first = playback();
  const stop = new QueuePersistence(first.player, "alice").start();
  await first.player.playQueue(tracks, 1);
  await vi.waitFor(() => expect(stored("alice")).not.toBeNull());
  stop();

  const credited: Track[] = tracks.map((track) => ({
    ...track,
    credited_artists: [
      { id: "member-a", name: "Member A", role: "main" },
      { id: "member-b", name: "Member B", role: "featured" },
    ],
  }));
  const second = playback();
  await new QueuePersistence(second.player, "alice", {
    lookup: library(credited),
  }).restore(new AbortController().signal);

  expect(
    second.player.snapshot().current?.credited_artists?.map((m) => m.id),
  ).toEqual(["member-a", "member-b"]);
});
