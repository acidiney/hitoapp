import { expect, test, vi } from "vitest";
import { audiobookPages } from "../../fixtures/audiobooks";
import { NativePlayback } from "./NativePlayback";
import { nativeService } from "./fixtures/service";

test("native Continue keeps current playback until one exact saved chapter commit", async () => {
  vi.spyOn(console, "info").mockImplementation(() => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.spyOn(console, "debug").mockImplementation(() => {});
  const { detail: page } = audiobookPages();
  const chapters = page.detail.chapters.slice(0, 3);
  const target = {
    audiobook_id: page.detail.playlist.id,
    entry_id: chapters[2].entry_id,
    recording_id: chapters[2].recording_id!,
  };
  const token = "429fdd84-c822-4a29-b659-d5b7fbf3ebac";
  const service = nativeService();
  Object.assign(service.state, {
    revision: 7,
    selection: 4,
    queue_len: 1,
    current_index: 0,
    current: { id: "old-entry", recording_id: "old-recording" },
    phase: "ready",
    desired_playing: true,
    playing: true,
    speed: 1500,
  });
  let committed = false;
  const entries = chapters.map((chapter, index) => ({
    id: `native-occurrence-${index}`,
    recording: { ...chapter, id: chapter.recording_id! },
    chapter: {
      audiobook_id: page.detail.playlist.id,
      entry_id: chapter.entry_id,
      recording_id: chapter.recording_id!,
    },
  }));
  service.queue = async (query) =>
    Response.json({
      revision: query.revision,
      total: committed ? entries.length : 1,
      offset: query.offset,
      entries: committed
        ? entries.slice(query.offset, query.offset + query.limit)
        : [
            {
              id: "old-entry",
              recording: {
                id: "old-recording",
                title: "Private current audio",
                artist: "Private narrator",
                duration_ms: 120000,
                available: true,
              },
            },
          ],
    });
  service.command = async (command) => {
    expect(command.action).toEqual({ kind: "continue_audiobook", token });
    expect(command).toMatchObject({ sequence: 1, revision: 7 });
    committed = true;
    Object.assign(service.state, {
      revision: 8,
      selection: 5,
      queue_len: 3,
      current_index: 2,
      current: {
        id: entries[2].id,
        recording_id: target.recording_id,
        chapter: target,
      },
      speed: 750,
    });
    service.emit("state", service.state);
    service.emit("position", {
      selection: 5,
      entry: entries[2].id,
      position_ms: 35125,
      duration_ms: 120000,
      seek_id: command.id,
    });
    return Response.json({
      id: command.id,
      sequence: command.sequence,
      revision: 8,
    });
  };
  const nativeRequest = vi.mocked(fetch).getMockImplementation()!;
  const requests: string[] = [];
  const preparations: { body: unknown; signal: AbortSignal }[] = [];
  let release!: () => void;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
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
        await held;
        return Response.json({
          token,
          chapter: target,
          position_ms: 35125,
          speed: 750,
          count: 3,
        });
      }
      return nativeRequest(path, options);
    }),
  );
  const player = new NativePlayback(page.user.id);
  const controller = new AbortController();
  try {
    await player.reconnect();
    await vi.waitFor(() =>
      expect(player.snapshot().current?.id).toBe("old-recording"),
    );
    const before = {
      playback: player.snapshot(),
      queue: player.queueSnapshot(),
      position: player.positionSnapshot(),
    };
    const continued = player.continueAudiobook(
      page.detail.playlist.id,
      controller.signal,
    );
    await vi.waitFor(() => expect(preparations).toHaveLength(1));
    expect(preparations[0].body).toEqual({
      id: expect.stringMatching(
        /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/,
      ),
      lease: "lease-1",
      revision: 7,
      selection: 4,
      audiobook_id: page.detail.playlist.id,
    });
    expect(preparations[0].signal).toBeInstanceOf(AbortSignal);
    expect(preparations[0].signal.aborted).toBe(false);
    expect(service.commands).toEqual([]);
    expect(player.snapshot()).toEqual(before.playback);
    expect(player.queueSnapshot()).toEqual(before.queue);
    expect(player.positionSnapshot()).toEqual(before.position);
    release();
    expect(await continued).toBe(true);
    expect(service.commands).toHaveLength(1);
    await vi.waitFor(() =>
      expect(player.snapshot().current?.id).toBe(target.recording_id),
    );
    expect(player.queueSnapshot()).toMatchObject({
      total: 3,
      currentIndex: 2,
      currentEntry: entries[2].id,
    });
    expect(player.snapshot().speed).toBe(0.75);
    expect(player.positionSnapshot()).toEqual({
      position: 35.125,
      duration: 120,
    });
    const queue = await player.queuePage(0, 10);
    expect(queue.entries.map(({ chapter }) => chapter?.entry_id)).toEqual(
      chapters.map(({ entry_id }) => entry_id),
    );
    expect(queue.entries[0].recording.id).toBe(queue.entries[2].recording.id);
    expect(queue.entries[0].id).not.toBe(queue.entries[2].id);
    expect(queue.entries[2].chapter).toEqual(target);
    expect(queue.entries[2].id).not.toBe(target.entry_id);
    expect(requests.every((path) => path.startsWith("/api/v1/player"))).toBe(
      true,
    );
    expect(document.querySelectorAll("audio")).toHaveLength(0);
    expect(player.snapshot().error).toBe("");
  } finally {
    release();
    controller.abort();
    player.close();
  }
});
