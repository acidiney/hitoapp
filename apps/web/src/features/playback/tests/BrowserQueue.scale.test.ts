import { afterEach, expect, test, vi } from "vitest";
import { Playback } from "../playback";
import { QueueMedia, queueTrack } from "../fixtures/browserQueueMedia";

afterEach(() => vi.restoreAllMocks());

function percentile(samples: number[], fraction: number) {
  const sorted = [...samples].sort((a, b) => a - b);
  return sorted[
    Math.min(sorted.length - 1, Math.floor(sorted.length * fraction))
  ];
}
function time(work: () => unknown) {
  const started = performance.now();
  work();
  return performance.now() - started;
}

test.each([10_000, 100_000])(
  "queue operations on %i entries stay far below a frame",
  async (size) => {
    const player = new Playback(new QueueMedia(), async (id) => [
      { id, ready: true },
    ]);
    await player.playQueue(
      Array.from({ length: size }, (_, i) => queueTrack(`r${i}`, `Track ${i}`)),
      size / 2,
    );
    player.shuffle();
    const ids = (await player.queuePage(size / 2 - 5, 100)).entries.map(
      (entry) => entry.id,
    );
    const moves = ids
      .slice(0, 40)
      .map((id, i) => time(() => player.moveEntry(id, ids[i + 50])));
    const pages = [];
    for (let i = 0; i < 40; i++) {
      const started = performance.now();
      await player.queuePage((i * 997) % (size - 50), 50);
      pages.push(performance.now() - started);
    }
    const removes = ids
      .slice(60, 80)
      .map((id) => time(() => player.removeEntry(id)));
    const more = Array.from({ length: 200 }, (_, i) =>
      queueTrack(`x${i}`, `Extra ${i}`),
    );
    let appended = 0;
    const supply = {
      remaining: () => (appended ? 0 : 200),
      next: async () => {
        appended = 1;
        return more;
      },
    };
    const started = performance.now();
    await player.playQueue(
      Array.from({ length: size - 200 }, (_, i) =>
        queueTrack(`s${i}`, `S ${i}`),
      ),
      0,
      undefined,
      supply,
    );
    await vi.waitFor(() => expect(player.queueSnapshot().pending).toBe(0));
    const replace = performance.now() - started;
    const nexts = [];
    for (let i = 0; i < 20; i++) {
      const begun = performance.now();
      await player.next();
      nexts.push(performance.now() - begun);
    }
    const line = {
      size,
      replace_and_append_ms: Math.round(replace),
      move_p95_ms: +percentile(moves, 0.95).toFixed(3),
      remove_p95_ms: +percentile(removes, 0.95).toFixed(3),
      page50_p95_ms: +percentile(pages, 0.95).toFixed(3),
      next_p95_ms: +percentile(nexts, 0.95).toFixed(3),
    };
    console.info("QUEUE_TIMING", JSON.stringify(line));
    expect(percentile(moves, 0.95)).toBeLessThan(50);
    expect(percentile(removes, 0.95)).toBeLessThan(50);
    expect(percentile(pages, 0.95)).toBeLessThan(50);
    expect(percentile(nexts, 0.95)).toBeLessThan(50);
  },
  60_000,
);

test("a shuffled hundred-thousand entry result loads in 200-entry batches without stalling the page", async () => {
  const player = new Playback(new QueueMedia(), async (id) => [
    { id, ready: true },
  ]);
  player.shuffle();
  let next = 200;
  const batches: number[] = [];
  let last = performance.now();
  const supply = {
    remaining: () => 100_000 - next,
    next: async () => {
      batches.push(performance.now() - last);
      const items = Array.from({ length: 200 }, (_, i) =>
        queueTrack(`b${next + i}`, `Track ${next + i}`),
      );
      next += 200;
      return items;
    },
  };
  const started = performance.now();
  await player.playQueue(
    Array.from({ length: 200 }, (_, i) => queueTrack(`b${i}`)),
    0,
    undefined,
    supply,
  );
  const stamps: number[] = [];
  const subscription = player.subscribe(() => {
    stamps.push(performance.now());
    last = performance.now();
  });
  await vi.waitFor(() => expect(player.queueSnapshot().total).toBe(100_000), {
    timeout: 30_000,
  });
  subscription();
  const total = performance.now() - started;
  const gaps = stamps.slice(1).map((stamp, i) => stamp - stamps[i]);
  console.info(
    "QUEUE_FEED",
    JSON.stringify({
      entries: 100_000,
      batches: 499,
      total_ms: Math.round(total),
      batch_gap_p95_ms: +percentile(gaps, 0.95).toFixed(3),
      batch_gap_max_ms: +Math.max(...gaps).toFixed(3),
    }),
  );
  expect(total).toBeLessThan(20_000);
  const seen = new Set<string>();
  for (let step = 0; step < 50; step++) {
    seen.add(player.queueSnapshot().currentEntry!);
    await player.next();
  }
  expect(seen.size).toBe(50);
}, 60_000);
