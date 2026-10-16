import { afterEach, expect, test, vi } from "vitest";
import { NativePlayback } from "./NativePlayback";
import { nativeService } from "./fixtures/service";

afterEach(() => vi.unstubAllGlobals());

test("native actions send IDs and controls while keeping source resolution and progress in Rust", async () => {
  const service = nativeService();
  const player = new NativePlayback("alice");
  try {
    await player.reconnect();
    await player.playQueue(
      [
        {
          id: "recording",
          title: "Private title",
          artist: "Private artist",
          duration_ms: 10000,
          available: true,
        },
      ],
      0,
      2.5,
    );
    player.seek(0);
    player.volume(0.25);
    await player.selectEntry("stable-entry");
    expect(service.commands.map((command) => command.action)).toEqual([
      { kind: "pause" },
      { kind: "replace", recordings: ["recording"], start: 0 },
      { kind: "seek", position_ms: 2500 },
      { kind: "resume" },
      { kind: "seek", position_ms: 0 },
      { kind: "volume", level: 250 },
      { kind: "select", entry: "stable-entry" },
      { kind: "resume" },
    ]);
    const prepared = player.prepareSelection();
    await player.next();
    expect(
      await prepared({
        id: "late",
        title: "Late",
        artist: "Fixture",
        duration_ms: 1,
        available: true,
      }),
    ).toBe(false);
    expect(service.commands.at(-1)?.action).toEqual({ kind: "next" });
    expect(JSON.stringify(service.commands)).not.toContain("Private");
    expect(
      service.requests.every((url) => url.startsWith("/api/v1/player")),
    ).toBe(true);
    const count = service.commands.length;
    player.speed(1.5);
    expect(service.commands).toHaveLength(count);
    expect(player.snapshot().speed).toBe(1);
    expect(player.snapshot().error).toContain("speed");
  } finally {
    player.close();
  }
});

test.each([0, 512, 1002])(
  "large selections at %i finalize bounded batches before a later Next intention",
  async (start) => {
    const service = nativeService();
    service.state.queue_len = 1025;
    service.state.shuffle = true;
    const player = new NativePlayback("alice");
    try {
      await player.reconnect();
      const tracks = Array.from({ length: 1025 }, (_, index) => ({
        id: `recording-${index}`,
        title: `Track ${index}`,
        artist: "Fixture",
        duration_ms: 10000,
        available: true,
      }));
      const play = player.playQueue(tracks, start);
      const next = player.next();
      await Promise.all([play, next]);
      expect(service.commands.map((command) => command.action.kind)).toEqual([
        "pause",
        "replace",
        "append",
        "select",
        "resume",
        "next",
      ]);
      expect(service.commands[1].action).toMatchObject({
        recordings: tracks.slice(0, 1000).map((track) => track.id),
        start: start < 1000 ? start : 0,
      });
      expect(service.commands[2].action).toEqual({
        kind: "append",
        recordings: tracks.slice(1000).map((track) => track.id),
      });
      expect(service.commands[3].action).toEqual({
        kind: "select",
        entry: `entry-${start}`,
      });
      expect(service.commands.map((command) => command.sequence)).toEqual([
        1, 2, 3, 4, 5, 6,
      ]);
    } finally {
      player.close();
    }
  },
);
