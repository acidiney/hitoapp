import { afterEach, expect, test, vi } from "vitest";
import { Playback } from "../playback";
import {
  MAX_QUEUE_ENTRIES,
  type PlaybackQueueInput,
  type QueueSupply,
} from "../model";
import { QueueMedia, queueTrack } from "../fixtures/browserQueueMedia";
import { deferred } from "../../../test/async";

afterEach(() => vi.restoreAllMocks());

function pagedSupply(total: number, size: number, from: number) {
  let next = from;
  const log = { active: 0, peak: 0, calls: 0, signals: [] as AbortSignal[] };
  const gates: ReturnType<typeof deferred<void>>[] = [];
  const supply: QueueSupply & { hold(): { resolve(): void } } = {
    remaining: () => Math.max(0, total - next),
    async next(signal) {
      log.calls++;
      log.signals.push(signal);
      log.active++;
      log.peak = Math.max(log.peak, log.active);
      try {
        const gate = gates.shift();
        if (gate) await gate.promise;
        signal.throwIfAborted();
        const items: PlaybackQueueInput[] = Array.from(
          { length: Math.min(size, total - next) },
          (_, i) => queueTrack(`r${next + i}`, `Track ${next + i}`),
        );
        next += items.length;
        return items;
      } finally {
        log.active--;
      }
    },
    hold() {
      const gate = deferred<void>();
      gates.push(gate);
      return gate;
    },
  };
  return { supply, log };
}

function build() {
  const media = new QueueMedia();
  const player = new Playback(media, async (id) => [{ id, ready: true }]);
  return { media, player };
}
const first = (count: number) =>
  Array.from({ length: count }, (_, i) => queueTrack(`r${i}`, `Track ${i}`));

test("later pages are fetched one at a time and appended behind the playing entry", async () => {
  const { player } = build();
  const { supply, log } = pagedSupply(1050, 200, 50);
  await player.playQueue(first(50), 0, undefined, supply);
  await vi.waitFor(() => expect(player.queueSnapshot().total).toBe(1050));
  expect(log.peak).toBe(1);
  expect(log.calls).toBe(5);
  expect(player.queueSnapshot().pending).toBe(0);
  expect(player.queueSnapshot().currentIndex).toBe(0);
  const last = await player.queuePage(1049, 1);
  expect(last.entries[0].recording.title).toBe("Track 1049");
  expect(player.snapshot().playing).toBe(true);
});

test("pending counts down while pages arrive and the first page plays immediately", async () => {
  const { player } = build();
  const { supply } = pagedSupply(300, 100, 100);
  const gate = supply.hold();
  await player.playQueue(first(100), 0, undefined, supply);
  expect(player.snapshot().playing).toBe(true);
  expect(player.queueSnapshot()).toMatchObject({ total: 100, pending: 200 });
  gate.resolve();
  await vi.waitFor(() => expect(player.queueSnapshot().total).toBe(300));
  expect(player.queueSnapshot().pending).toBe(0);
});

test("a hard cap stops fetching at the queue limit and says so", async () => {
  const { player } = build();
  const { supply } = pagedSupply(MAX_QUEUE_ENTRIES + 5000, 5000, 5000);
  await player.playQueue(first(5000), 0, undefined, supply);
  await vi.waitFor(() =>
    expect(player.queueSnapshot().total).toBe(MAX_QUEUE_ENTRIES),
  );
  await vi.waitFor(() => expect(player.snapshot().error).toMatch(/100,000/));
  expect(player.queueSnapshot().pending).toBe(0);
});

test("replacing the queue aborts the held fetch and drops its late page", async () => {
  const { player } = build();
  const { supply, log } = pagedSupply(400, 100, 100);
  const gate = supply.hold();
  await player.playQueue(first(100), 0, undefined, supply);
  await vi.waitFor(() => expect(log.calls).toBe(1));
  await player.playQueue([queueTrack("other", "Other")]);
  expect(log.signals[0].aborted).toBe(true);
  gate.resolve();
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(player.queueSnapshot()).toMatchObject({ total: 1, pending: 0 });
  expect(log.calls).toBe(1);
});

test("clearing and retiring the player abort fetching", async () => {
  for (const stop of ["clear", "retire"] as const) {
    const { player } = build();
    const { supply, log } = pagedSupply(400, 100, 100);
    supply.hold();
    await player.playQueue(first(100), 0, undefined, supply);
    await vi.waitFor(() => expect(log.calls).toBe(1));
    player[stop]();
    expect(log.signals[0].aborted).toBe(true);
    expect(player.queueSnapshot()).toMatchObject({ total: 0, pending: 0 });
  }
});

test("a failed page stops quietly with a message and keeps the loaded queue", async () => {
  const { player } = build();
  const supply: QueueSupply = {
    remaining: () => 100,
    next: async () => {
      throw new Error("offline");
    },
  };
  await player.playQueue(first(10), 0, undefined, supply);
  await vi.waitFor(() =>
    expect(player.snapshot().error).toMatch(/rest of the list/i),
  );
  expect(player.queueSnapshot()).toMatchObject({ total: 10, pending: 0 });
  expect(player.snapshot().playing).toBe(true);
});

test("shuffle mixes later pages into the unplayed remainder and visits each entry once", async () => {
  const { player } = build();
  await player.playQueue(first(100), 0);
  player.shuffle();
  const { supply } = pagedSupply(200, 100, 100);
  const entries = async () => (await player.queuePage(0, 1000)).entries;
  await player.playQueue(first(100), 0, undefined, supply);
  await vi.waitFor(() => expect(player.queueSnapshot().total).toBe(200));
  const titles = new Map(
    (await entries()).map((e) => [e.id, e.recording.title]),
  );
  const visited = [titles.get(player.queueSnapshot().currentEntry!)!];
  for (let step = 1; step < 200; step++) {
    await player.next();
    visited.push(titles.get(player.queueSnapshot().currentEntry!)!);
  }
  expect(new Set(visited).size).toBe(200);
  const loaded = new Set(first(100).map((t) => t.title));
  expect(visited.slice(1, 101).some((title) => !loaded.has(title))).toBe(true);
});

test("appending while the last entry plays prepares the new successor", async () => {
  const { player } = build();
  const { supply } = pagedSupply(2, 2, 1);
  await player.playQueue(first(1), 0, undefined, supply);
  await vi.waitFor(() => expect(player.queueSnapshot().total).toBe(2));
  await player.next();
  expect(player.snapshot().current?.title).toBe("Track 1");
});
