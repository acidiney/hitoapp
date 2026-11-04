import { expect, test, vi } from "vitest";
import { NativePlayback } from "../../native/NativePlayback";
import type { Snapshot } from "../../native/api";
import { nativeService } from "../../fixtures/native/service";

test("a device that cannot play the source says so instead of the generic playback failure", async () => {
  const service = nativeService();
  service.state.error = "output_format";
  const player = new NativePlayback("alice");
  try {
    await player.reconnect();
    await vi.waitFor(() =>
      expect(player.snapshot().error).toBe(
        "The audio output can't play this format.",
      ),
    );
  } finally {
    player.close();
  }
});

test("a queued track skipped for an unplayable format keeps the specific reason", async () => {
  const service = nativeService();
  service.state.last_failure = {
    entry: { id: "entry-1", recording_id: "recording-1" },
    error: "output_format",
  };
  const player = new NativePlayback("alice");
  try {
    await player.reconnect();
    await vi.waitFor(() =>
      expect(player.snapshot().error).toBe(
        "The audio output can't play a queued track's format.",
      ),
    );
  } finally {
    player.close();
  }
});

test("position reconnection preserves an unresolved Rust progress warning", async () => {
  const service = nativeService();
  service.state.persistence_error = "persistence";
  const player = new NativePlayback("alice");
  try {
    await player.reconnect();
    await vi.waitFor(() =>
      expect(player.snapshot().error).toContain("could not be saved"),
    );
    service.end("position");
    await vi.waitFor(() =>
      expect(player.snapshot().connection).toBe("disconnected"),
    );
    await vi.waitFor(() => expect(service.subscriptions.position).toBe(2));
    await vi.waitFor(() => expect(player.snapshot().connection).toBe("ready"));
    expect(player.snapshot().error).toContain("could not be saved");
    expect(service.claims).toBe(1);
  } finally {
    player.close();
  }
});

test("a position-only interruption recovers without claiming again or a structural change", async () => {
  const service = nativeService();
  const player = new NativePlayback("alice");
  try {
    await player.reconnect();
    await vi.waitFor(() => expect(player.snapshot().connection).toBe("ready"));
    service.end("position");
    await vi.waitFor(() =>
      expect(player.snapshot().connection).toBe("disconnected"),
    );
    await vi.waitFor(() => expect(service.subscriptions.position).toBe(2));
    await vi.waitFor(() => expect(player.snapshot().connection).toBe("ready"));
    expect(service.subscriptions.state).toBe(1);
    expect(service.claims).toBe(1);
  } finally {
    player.close();
  }
});

test("ownership rejection clears private views and drops later commands without reclaiming", async () => {
  const service = nativeService();
  Object.assign(service.state, {
    revision: 1,
    selection: 1,
    queue_len: 1,
    current_index: 0,
    current: { id: "entry-0", recording_id: "recording-0" },
  });
  service.command = async () => new Response(null, { status: 409 });
  const player = new NativePlayback("alice");
  try {
    await player.reconnect();
    await vi.waitFor(() =>
      expect(player.snapshot().current?.id).toBe("recording-0"),
    );
    await Promise.all([player.toggle(), player.next()]);
    expect(service.commands).toHaveLength(1);
    expect(service.claims).toBe(1);
    expect(player.snapshot().connection).toBe("disconnected");
    expect(player.snapshot().current).toBeNull();
    expect(player.queueSnapshot().total).toBe(0);
    expect(player.positionSnapshot().position).toBe(0);
  } finally {
    player.close();
  }
});

test("an old metadata response cannot replace a reattached selection", async () => {
  const service = nativeService();
  Object.assign(service.state, {
    revision: 1,
    selection: 1,
    queue_len: 2,
    current_index: 0,
    current: { id: "entry-0", recording_id: "recording-0" },
  });
  const queue = service.queue;
  let release!: () => void;
  const blocked = new Promise<void>((resolve) => {
    release = resolve;
  });
  let reads = 0;
  service.queue = async (query) => {
    if (++reads === 1) await blocked;
    return queue(query);
  };
  const player = new NativePlayback("alice");
  try {
    await player.reconnect();
    await vi.waitFor(() => expect(reads).toBe(1));
    Object.assign(service.state, {
      selection: 2,
      current_index: 1,
      current: { id: "entry-1", recording_id: "recording-1" },
    });
    await player.reconnect();
    await vi.waitFor(() =>
      expect(player.snapshot().current?.id).toBe("recording-1"),
    );
    release();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(player.snapshot().current?.id).toBe("recording-1");
    expect(player.queueSnapshot().currentEntry).toBe("entry-1");
    expect(service.claims).toBe(2);
  } finally {
    release();
    player.close();
  }
});

test("native views page a large queue and isolate position frames from queue and metadata state", async () => {
  const streams = new Map<
    string,
    ReadableStreamDefaultController<Uint8Array>
  >();
  const queries: { offset: number; limit: number }[] = [];
  const state: Snapshot = {
    revision: 1,
    selection: 1,
    queue_len: 100000,
    can_previous: true,
    shuffle: false,
    repeat: "off",
    current_index: 50123,
    current: { id: "entry-50123", recording_id: "recording-50123" },
    source_id: null,
    desired_playing: true,
    playing: true,
    seeking: false,
    volume: 750,
    speed: 1000,
    effective_speed: 1000,
    speed_support: null,
    phase: "ready",
    error: null,
    last_failure: null,
    persistence_error: null,
  };
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, options: RequestInit) => {
      if (url === "/api/v1/player")
        return Response.json({ native: true, epoch: "epoch", state: null });
      if (url.endsWith("/claim"))
        return Response.json({
          native: true,
          epoch: "attached",
          lease: "private-lease",
          state,
        });
      if (url.endsWith("/events")) {
        let cancelled = false;
        return new Response(
          new ReadableStream({
            cancel() {
              cancelled = true;
            },
            start(controller) {
              streams.set(
                url.includes("/position/") ? "position" : "state",
                controller,
              );
              options.signal?.addEventListener(
                "abort",
                () => {
                  if (!cancelled) controller.close();
                },
                { once: true },
              );
            },
          }),
          { headers: { "content-type": "text/event-stream" } },
        );
      }
      if (url.endsWith("/queue")) {
        const query = JSON.parse(String(options.body));
        queries.push(query);
        return Response.json({
          revision: 1,
          total: 100000,
          offset: query.offset,
          entries: Array.from({ length: query.limit }, (_, i) => ({
            id: `entry-${query.offset + i}`,
            recording: {
              id: `recording-${query.offset + i}`,
              title: `Track ${query.offset + i}`,
              artist: "Fixture",
              duration_ms: 10000,
              available: true,
            },
          })),
        });
      }
      throw new Error("Unexpected fixture request");
    }),
  );
  const player = new NativePlayback("alice");
  try {
    await player.reconnect();
    await vi.waitFor(() =>
      expect(player.snapshot().current?.id).toBe("recording-50123"),
    );
    expect(player.queueSnapshot().total).toBe(100000);
    const view = player.queueSnapshot();
    let changes = 0;
    player.subscribe(() => changes++);
    const emit = (value: unknown) =>
      streams
        .get("position")!
        .enqueue(
          new TextEncoder().encode(`data: ${JSON.stringify(value)}\n\n`),
        );
    emit({
      selection: 1,
      entry: "entry-50123",
      position_ms: 3500,
      duration_ms: 10000,
      seek_id: null,
    });
    await vi.waitFor(() =>
      expect(player.positionSnapshot().position).toBe(3.5),
    );
    expect(changes).toBe(0);
    expect(player.queueSnapshot()).toBe(view);
    const page = await player.queuePage(50100, 50);
    expect(page.entries).toHaveLength(50);
    expect(page.entries[23].id).toBe(view.currentEntry);
    expect(queries.map((query) => query.limit)).toEqual([1, 50]);
    emit({
      selection: 0,
      entry: "old-entry",
      position_ms: 9000,
      duration_ms: 10000,
      seek_id: null,
    });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(player.positionSnapshot().position).toBe(3.5);
    expect(player.snapshot().volume).toBe(0.75);
  } finally {
    player.close();
  }
  expect(player.snapshot().current).toBeNull();
  expect(player.queueSnapshot().total).toBe(0);
});
