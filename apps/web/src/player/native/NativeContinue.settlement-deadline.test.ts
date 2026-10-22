import { expect, test, vi } from "vitest";
import { audiobookPages } from "../../features/audiobooks/fixtures/audiobooks";
import { NativePlayback } from "./NativePlayback";
import { nativeService } from "./fixtures/service";
import type { Command } from "./api";

test.each(["stalled", "malformed"])(
  "a %s cancellation settlement cannot advance the uncertain command lane",
  async (mode) => {
    vi.spyOn(console, "info").mockImplementation(() => {});
    vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.spyOn(console, "debug").mockImplementation(() => {});
    const { detail: page } = audiobookPages();
    const chapter = page.detail.chapters[2];
    const service = nativeService();
    Object.assign(service.state, {
      revision: 7,
      selection: 4,
      queue_len: 1,
      current_index: 0,
      current: { id: "entry-0", recording_id: "recording-0" },
      phase: "ready",
      desired_playing: true,
      playing: true,
      speed: 1500,
    });
    const request = vi.mocked(fetch).getMockImplementation()!;
    let releaseCommand!: () => void;
    const held = new Promise<void>((resolve) => {
      releaseCommand = resolve;
    });
    let readBody = false;
    let body!: ReadableStreamDefaultController<Uint8Array>;
    let settlementSignal: AbortSignal | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn<typeof fetch>(async (path, options) => {
        if (path === "/api/v1/player/audiobooks/prepare")
          return Response.json({
            token: "429fdd84-c822-4a29-b659-d5b7fbf3ebac",
            chapter: {
              audiobook_id: page.detail.playlist.id,
              entry_id: chapter.entry_id,
              recording_id: chapter.recording_id,
            },
            position_ms: 35125,
            speed: 750,
            count: 3,
          });
        if (path === "/api/v1/player/audiobooks/cancel") {
          const input = JSON.parse(String(options?.body));
          if (!input.command) return new Response(null, { status: 204 });
          settlementSignal = options?.signal as AbortSignal;
          if (mode === "malformed") {
            readBody = true;
            return Response.json({ receipt: false });
          }
          return new Response(
            new ReadableStream<Uint8Array>({
              start(controller) {
                body = controller;
              },
              pull() {
                readBody = true;
              },
            }),
            { headers: { "content-type": "application/json" } },
          );
        }
        if (path === "/api/v1/player/commands") {
          const command: Command = JSON.parse(String(options?.body));
          service.commands.push(command);
          expect(command.action.kind).toBe("continue_audiobook");
          await held;
          return Response.json({
            id: command.id,
            sequence: command.sequence,
            revision: 8,
          });
        }
        return request(path, options);
      }),
    );
    const player = new NativePlayback(page.user.id);
    const controller = new AbortController();
    try {
      await player.reconnect();
      await vi.waitFor(() => expect(player.snapshot().playing).toBe(true));
      const before = player.queueSnapshot();
      vi.useFakeTimers();
      const continuing = player.continueAudiobook(
        page.detail.playlist.id,
        controller.signal,
      );
      await vi.waitFor(() => expect(service.commands).toHaveLength(1));
      const paused = player.setPlaying(false);
      await vi.waitFor(() => expect(readBody).toBe(true));
      await vi.advanceTimersByTimeAsync(0);
      expect(service.commands).toHaveLength(1);
      if (mode === "stalled") {
        expect(settlementSignal?.aborted).toBe(false);
        expect(player.queueSnapshot()).toEqual(before);
        await vi.advanceTimersByTimeAsync(9_000);
        expect(service.commands).toHaveLength(1);
        expect(player.snapshot().playing).toBe(true);
        await vi.advanceTimersByTimeAsync(1_000);
      }
      await paused;
      expect(await continuing).toBe(false);
      if (mode === "stalled") expect(settlementSignal?.aborted).toBe(true);
      expect(player.snapshot().connection).toBe("disconnected");
      expect(player.snapshot().error).toContain("Reconnect");
      expect(service.commands).toHaveLength(1);
      expect(service.claims).toBe(1);
      if (mode === "stalled") {
        body.enqueue(
          new TextEncoder().encode(
            JSON.stringify({
              receipt: { id: service.commands[0].id, sequence: 1, revision: 8 },
            }),
          ),
        );
        body.close();
      }
      releaseCommand();
      await Promise.resolve();
      expect(service.commands).toHaveLength(1);
      expect(player.snapshot().connection).toBe("disconnected");
    } finally {
      releaseCommand();
      controller.abort();
      player.close();
    }
  },
);
