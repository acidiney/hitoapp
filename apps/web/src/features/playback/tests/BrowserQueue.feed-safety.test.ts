import { afterEach, expect, test, vi } from "vitest";
import { Playback } from "../playback";
import type { QueueSupply } from "../model";
import type { SourceResolver } from "../api/playbackSources";
import { HttpError } from "../../../shared/http";
import { BrowserTraversal } from "../browser/BrowserTraversal";
import {
  QueueMedia,
  queueMediaOwner,
  queueTrack,
} from "../fixtures/browserQueueMedia";

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const first = (count: number) =>
  Array.from({ length: count }, (_, i) => queueTrack(`r${i}`, `Track ${i}`));

test("fifty shuffled batches never retarget the prepared next entry", async () => {
  const initial = new QueueMedia();
  const owner = queueMediaOwner(initial);
  const requests: { recording: string; signal: AbortSignal }[] = [];
  const resolve: SourceResolver = async (recording, options) => {
    requests.push({ recording, signal: options.signal });
    if (recording === "r0") return [{ id: "audio-r0", ready: true }];
    return new Promise(() => {});
  };
  const player = new Playback(initial, resolve, undefined, owner);
  player.shuffle();
  let batch = 0;
  const supply: QueueSupply = {
    remaining: () => 50 * 20 - batch * 20,
    next: async () => {
      const base = batch++ * 20;
      return Array.from({ length: 20 }, (_, i) =>
        queueTrack(`b${base + i}`, `B ${base + i}`),
      );
    },
  };
  await player.playQueue(first(10), 0, undefined, supply);
  await vi.waitFor(() => expect(requests.length).toBeGreaterThan(1));
  const next = requests.find((r) => r.recording !== "r0")!;
  await vi.waitFor(() => expect(player.queueSnapshot().total).toBe(1010));
  expect(next.signal.aborted).toBe(false);
  expect(requests.filter((r) => r.recording === next.recording)).toHaveLength(
    1,
  );
});

test("entries appended after the queue ended mix only with each other", () => {
  for (let trial = 0; trial < 40; trial++) {
    const traversal = new BrowserTraversal();
    traversal.toggle([1, 2, 3], 1);
    traversal.append([4, 5, 6], undefined);
    const all = [1, 2, 3, 4, 5, 6];
    let current = all.indexOf(1);
    const visited = [1];
    for (let step = 0; step < 5; step++) {
      const next = traversal.next(all, current, "off", "completed");
      if (next === null) break;
      current = next;
      visited.push(all[next]);
    }
    expect(visited.slice(3).every((entry) => entry > 3)).toBe(true);
  }
});

function failing(errors: unknown[], remaining = 100) {
  let calls = 0;
  const supply: QueueSupply = {
    remaining: () => remaining,
    next: async () => {
      const error = errors[calls++];
      if (error) throw error;
      remaining = 0;
      return [queueTrack("late", "Late")];
    },
  };
  return { supply, calls: () => calls };
}

test("a throttled page is retried after the server's Retry-After and then appended", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout"] });
  const player = new Playback(new QueueMedia(), async (id) => [
    { id, ready: true },
  ]);
  const slow = new HttpError(429, "slow down");
  Object.assign(slow, { retryAfter: 2 });
  const { supply, calls } = failing([slow]);
  await player.playQueue(first(2), 0, undefined, supply);
  await vi.advanceTimersByTimeAsync(1999);
  expect(calls()).toBe(1);
  await vi.advanceTimersByTimeAsync(2);
  await vi.waitFor(() => expect(player.queueSnapshot().total).toBe(3));
  expect(player.snapshot().error).toBe("");
});

test("pages are retried a bounded number of times, then the list stops with the visible message and a safe log", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout"] });
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const player = new Playback(new QueueMedia(), async (id) => [
    { id, ready: true },
  ]);
  const { supply, calls } = failing(
    Array(10).fill(new HttpError(503, "private body")),
  );
  await player.playQueue(first(2), 0, undefined, supply);
  await vi.advanceTimersByTimeAsync(60_000);
  await vi.waitFor(() =>
    expect(player.snapshot().error).toBe(
      "The rest of the list could not be loaded. Play it again to retry.",
    ),
  );
  expect(calls()).toBe(4);
  const logged = warn.mock.calls.find(
    ([, f]) => f?.event === "queue.feed_failed",
  )![1];
  expect(logged).toMatchObject({ status: 503, count: 0 });
  expect(JSON.stringify(logged)).not.toContain("private body");
});

test("a rejected request is not retried", async () => {
  const player = new Playback(new QueueMedia(), async (id) => [
    { id, ready: true },
  ]);
  const { supply, calls } = failing([new HttpError(404, "gone")]);
  await player.playQueue(first(2), 0, undefined, supply);
  await vi.waitFor(() =>
    expect(player.snapshot().error).toMatch(/rest of the list/),
  );
  expect(calls()).toBe(1);
});

test("a no-op or invalid move leaves audiobook continuation alone", async () => {
  const player = new Playback(new QueueMedia(), async (id) => [
    { id, ready: true },
  ]);
  await player.playQueue(first(3), 0);
  const cancel = vi.spyOn(player as never, "cancelAudiobook" as never);
  const ids = (await player.queuePage(0, 3)).entries.map((e) => e.id);
  player.moveEntry(ids[0], ids[0]);
  player.moveEntry("missing", ids[1]);
  player.moveEntry(ids[1], ids[2]);
  expect(cancel).not.toHaveBeenCalled();
  player.moveEntry(ids[2], ids[0]);
  expect(cancel).toHaveBeenCalledTimes(1);
});
