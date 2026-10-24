import { expect, test, vi } from "vitest";
import { NativePlayback } from "../../native/NativePlayback";
import { nativeService } from "../../fixtures/native/service";
import type { PlaybackQueueInput, QueueSupply } from "../../model";
import { deferred } from "../../../../test/async";

const track = (index: number): PlaybackQueueInput => ({
  id: `recording-${index}`,
  title: `Track ${index}`,
  artist: "Fixture",
  duration_ms: 10000,
  available: true,
});

function supply(total: number, size: number, from: number) {
  let next = from;
  const signals: AbortSignal[] = [];
  const gates: ReturnType<typeof deferred<void>>[] = [];
  const value: QueueSupply = {
    remaining: () => Math.max(0, total - next),
    async next(signal) {
      signals.push(signal);
      await gates.shift()?.promise;
      signal.throwIfAborted();
      const items = Array.from(
        { length: Math.min(size, total - next) },
        (_, i) => track(next + i),
      );
      next += items.length;
      return items;
    },
  };
  return {
    value,
    signals,
    hold() {
      const gate = deferred<void>();
      gates.push(gate);
      return gate;
    },
  };
}

test("moving sends the entry and its successor through the existing command path without selecting", async () => {
  const service = nativeService();
  const player = new NativePlayback("alice");
  try {
    await player.reconnect();
    player.moveEntry("entry-3", "entry-1");
    player.moveEntry("entry-1", null);
    await vi.waitFor(() => expect(service.commands).toHaveLength(2));
    expect(service.commands.map((command) => command.action)).toEqual([
      { kind: "move", entry: "entry-3", before: "entry-1" },
      { kind: "move", entry: "entry-1", before: null },
    ]);
    expect(service.commands.map((command) => command.sequence)).toEqual([1, 2]);
  } finally {
    player.close();
  }
});

test("later pages are appended after the replace in order while the first page plays", async () => {
  const service = nativeService();
  const player = new NativePlayback("alice");
  try {
    await player.reconnect();
    const more = supply(450, 200, 50);
    await player.playQueue(
      Array.from({ length: 50 }, (_, i) => track(i)),
      0,
      undefined,
      more.value,
    );
    await vi.waitFor(() =>
      expect(service.commands.map((c) => c.action.kind)).toEqual([
        "pause",
        "replace",
        "resume",
        "append",
        "append",
      ]),
    );
    const appended = service.commands
      .slice(3)
      .flatMap((c) => (c.action.kind === "append" ? c.action.recordings : []));
    expect(appended).toEqual(
      Array.from({ length: 400 }, (_, i) => `recording-${50 + i}`),
    );
    expect(player.queueSnapshot().pending).toBe(0);
  } finally {
    player.close();
  }
});

test("a new selection, clear or close aborts fetching and never appends to the new queue", async () => {
  for (const stop of ["play", "clear", "close"] as const) {
    const service = nativeService();
    const player = new NativePlayback("alice");
    try {
      await player.reconnect();
      const more = supply(400, 100, 100);
      const gate = more.hold();
      await player.playQueue(
        Array.from({ length: 100 }, (_, i) => track(i)),
        0,
        undefined,
        more.value,
      );
      await vi.waitFor(() => expect(more.signals).toHaveLength(1));
      expect(player.queueSnapshot().pending).toBe(300);
      if (stop === "play") await player.playQueue([track(900)]);
      else if (stop === "clear") player.clear();
      else player.close();
      expect(more.signals[0].aborted).toBe(true);
      const sent = service.commands.length;
      gate.resolve();
      await new Promise((resolve) => setTimeout(resolve, 20));
      expect(
        service.commands.slice(sent).some((c) => c.action.kind === "append"),
      ).toBe(false);
      expect(player.queueSnapshot().pending).toBe(0);
    } finally {
      player.close();
    }
  }
});

test("a failed page leaves the loaded queue and reports the rest as not loaded", async () => {
  const service = nativeService();
  const player = new NativePlayback("alice");
  try {
    await player.reconnect();
    await player.playQueue([track(0)], 0, undefined, {
      remaining: () => 10,
      next: async () => {
        throw new Error("offline");
      },
    });
    await vi.waitFor(() =>
      expect(player.snapshot().error).toMatch(/rest of the list/i),
    );
    expect(service.commands.some((c) => c.action.kind === "append")).toBe(
      false,
    );
    expect(player.queueSnapshot().pending).toBe(0);
  } finally {
    player.close();
  }
});

test("a move whose target vanished and a conflicting session each get their own message", async () => {
  for (const [status, message] of [
    [404, /removed/i],
    [409, /ownership changed/i],
  ] as const) {
    const service = nativeService();
    const player = new NativePlayback("alice");
    try {
      await player.reconnect();
      service.command = async () =>
        new Response(JSON.stringify({ error: "private detail" }), { status });
      player.moveEntry("entry-3", "entry-1");
      await vi.waitFor(() => expect(player.snapshot().error).toMatch(message));
      expect(player.snapshot().error).not.toContain("private detail");
    } finally {
      player.close();
    }
  }
});

test("a rejected append stops the feed with the session-ownership message and is not repeated", async () => {
  const service = nativeService();
  const player = new NativePlayback("alice");
  try {
    await player.reconnect();
    const more = supply(450, 200, 50);
    const accept = service.command;
    service.command = async (command) =>
      command.action.kind === "append"
        ? new Response("{}", { status: 409 })
        : accept(command);
    await player.playQueue(
      Array.from({ length: 50 }, (_, i) => track(i)),
      0,
      undefined,
      more.value,
    );
    await vi.waitFor(() =>
      expect(player.snapshot().error).toMatch(/ownership changed/i),
    );
    expect(
      service.commands.filter((c) => c.action.kind === "append"),
    ).toHaveLength(1);
    expect(player.queueSnapshot().pending).toBe(0);
  } finally {
    player.close();
  }
});
