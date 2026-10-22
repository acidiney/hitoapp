import { expect, test, vi } from "vitest";
import { audiobookPages } from "../../../audiobooks/fixtures/audiobooks";
import { NativePlayback } from "../../native/NativePlayback";
import { nativeService } from "../../fixtures/native/service";

test("a newer native Pause retires held Continue without reserving or blocking its command", async () => {
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
  let releasePause!: () => void;
  const pauseReceipt = new Promise<void>((resolve) => {
    releasePause = resolve;
  });
  service.command = async (command) => {
    expect(command.action).toEqual({ kind: "pause" });
    await pauseReceipt;
    Object.assign(service.state, { desired_playing: false, playing: false });
    service.emit("state", service.state);
    return Response.json({
      id: command.id,
      sequence: command.sequence,
      revision: command.revision,
    });
  };
  const nativeRequest = vi.mocked(fetch).getMockImplementation()!;
  const requests: string[] = [];
  const preparations: { body: unknown; signal: AbortSignal }[] = [];
  const cancellations: unknown[] = [];
  let releasePrepare!: () => void;
  const prepareRead = new Promise<void>((resolve) => {
    releasePrepare = resolve;
  });
  let lateResponseReturned = false;
  vi.stubGlobal(
    "fetch",
    vi.fn<typeof globalThis.fetch>(async (path, options) => {
      requests.push(String(path));
      expect(options?.credentials).toBe("same-origin");
      expect(new Headers(options?.headers).get("x-hito-account")).toBe(
        page.user.id,
      );
      if (path === "/api/v1/player/audiobooks/prepare") {
        expect(options?.method).toBe("POST");
        preparations.push({
          body: JSON.parse(String(options?.body)),
          signal: options?.signal as AbortSignal,
        });
        await prepareRead;
        lateResponseReturned = true;
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
      }
      if (path === "/api/v1/player/audiobooks/cancel") {
        expect(options?.method).toBe("POST");
        expect(options?.signal?.aborted).toBe(false);
        cancellations.push(JSON.parse(String(options?.body)));
        return new Response(null, { status: 204 });
      }
      return nativeRequest(path, options);
    }),
  );
  const player = new NativePlayback(page.user.id);
  const controller = new AbortController();
  try {
    await player.reconnect();
    await vi.waitFor(() =>
      expect(player.snapshot().current?.id).toBe("recording-0"),
    );
    const before = {
      playback: player.snapshot(),
      queue: player.queueSnapshot(),
      position: player.positionSnapshot(),
    };
    const continuing = player.continueAudiobook(
      page.detail.playlist.id,
      controller.signal,
    );
    await vi.waitFor(() => expect(preparations).toHaveLength(1));
    const paused = player.setPlaying(false);
    await vi.waitFor(() => expect(service.commands).toHaveLength(1));
    expect(service.commands[0]).toMatchObject({
      sequence: 1,
      revision: 7,
      action: { kind: "pause" },
    });
    expect(await continuing).toBe(false);
    await vi.waitFor(() => expect(cancellations).toHaveLength(1));
    expect(cancellations[0]).toEqual({
      lease: "lease-1",
      id: (preparations[0].body as { id: string }).id,
    });
    expect(preparations[0].signal.aborted).toBe(true);
    expect(lateResponseReturned).toBe(false);
    expect(player.snapshot()).toEqual(before.playback);
    expect(player.queueSnapshot()).toEqual(before.queue);
    expect(player.positionSnapshot()).toEqual(before.position);
    releasePrepare();
    await vi.waitFor(() => expect(lateResponseReturned).toBe(true));
    expect(service.commands).toHaveLength(1);
    releasePause();
    await paused;
    await vi.waitFor(() => expect(player.snapshot().playing).toBe(false));
    expect(player.queueSnapshot()).toEqual(before.queue);
    expect(player.snapshot().current?.id).toBe("recording-0");
    expect(player.snapshot().speed).toBe(1.5);
    expect(player.positionSnapshot()).toEqual(before.position);
    expect(player.snapshot().error).toBe("");
    expect(service.commands).toHaveLength(1);
    expect(service.claims).toBe(1);
    expect(requests.every((path) => path.startsWith("/api/v1/player"))).toBe(
      true,
    );
    expect(document.querySelectorAll("audio")).toHaveLength(0);
  } finally {
    releasePrepare();
    releasePause();
    controller.abort();
    player.close();
  }
});
