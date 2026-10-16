import { afterEach, expect, test, vi } from "vitest";
import { NativePlayback } from "./NativePlayback";
import type { Snapshot } from "./api";

afterEach(() => vi.unstubAllGlobals());

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
    current_index: 50123,
    current: { id: "entry-50123", recording_id: "recording-50123" },
    source_id: null,
    desired_playing: true,
    playing: true,
    seeking: false,
    volume: 750,
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
      if (url.endsWith("/events"))
        return new Response(
          new ReadableStream({
            start(controller) {
              streams.set(
                url.includes("/position/") ? "position" : "state",
                controller,
              );
              options.signal?.addEventListener(
                "abort",
                () => controller.close(),
                { once: true },
              );
            },
          }),
          { headers: { "content-type": "text/event-stream" } },
        );
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
