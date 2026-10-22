import { expect, test, vi } from "vitest";
import { audiobookPages } from "../../../audiobooks/fixtures/audiobooks";
import { NativePlayback } from "../../native/NativePlayback";
import { nativeService } from "../../fixtures/native/service";
import type { Command } from "../../native/api";

test.each([false, true])(
  "Pause settles final audiobook admission before its ordered command, retry backoff %s",
  async (backoff) => {
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
    let releaseAdmission!: () => void;
    const admission = new Promise<void>((resolve) => {
      releaseAdmission = resolve;
    });
    let heldSignal: AbortSignal | undefined;
    let heldCommand: Command | undefined;
    let cancelled = false;
    const cancellations: unknown[] = [];
    const request = vi.mocked(fetch).getMockImplementation()!;
    vi.stubGlobal(
      "fetch",
      vi.fn<typeof fetch>(async (path, options) => {
        expect(new Headers(options?.headers).get("x-hito-account")).toBe(
          page.user.id,
        );
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
          const body = JSON.parse(String(options?.body));
          cancellations.push(body);
          expect(body.command).toEqual(heldCommand);
          cancelled = true;
          return Response.json({ receipt: null });
        }
        if (path === "/api/v1/player/commands") {
          const command: Command = JSON.parse(String(options?.body));
          if (command.action.kind === "continue_audiobook") {
            service.commands.push(command);
            heldCommand = command;
            heldSignal = options?.signal as AbortSignal;
            if (backoff)
              throw new TypeError("Owned fixture connection interrupted");
            await admission;
            expect(cancelled).toBe(true);
            return Response.json(
              { message: "The continuation was retired" },
              { status: 409 },
            );
          }
          expect(command).toMatchObject({
            sequence: 2,
            revision: 7,
            action: { kind: "pause" },
          });
          expect(cancelled).toBe(true);
        }
        return request(path, options);
      }),
    );
    service.command = async (command) => {
      Object.assign(service.state, { playing: false, desired_playing: false });
      service.emit("state", service.state);
      return Response.json({
        id: command.id,
        sequence: command.sequence,
        revision: command.revision,
      });
    };
    const player = new NativePlayback(page.user.id);
    const controller = new AbortController();
    try {
      await player.reconnect();
      await vi.waitFor(() => expect(player.snapshot().playing).toBe(true));
      const before = player.queueSnapshot();
      const continuing = player.continueAudiobook(
        page.detail.playlist.id,
        controller.signal,
      );
      await vi.waitFor(() => expect(heldCommand).toBeDefined());
      const paused = player.setPlaying(false);
      await vi.waitFor(() => expect(heldSignal?.aborted).toBe(true));
      await vi.waitFor(() => expect(cancellations).toHaveLength(1));
      expect(cancellations[0]).toMatchObject({ command: heldCommand });
      await vi.waitFor(() => expect(service.commands).toHaveLength(2));
      expect(player.queueSnapshot()).toEqual(before);
      releaseAdmission();
      expect(await continuing).toBe(false);
      await paused;
      await vi.waitFor(() => expect(player.snapshot().playing).toBe(false));
      expect(service.commands).toHaveLength(2);
      expect(player.snapshot().current?.id).toBe("recording-0");
      expect(player.snapshot().speed).toBe(1.5);
      expect(player.queueSnapshot()).toEqual(before);
      expect(player.snapshot().error).toBe("");
      expect(service.claims).toBe(1);
      expect(document.querySelectorAll("audio")).toHaveLength(0);
    } finally {
      releaseAdmission();
      controller.abort();
      player.close();
    }
  },
);

test("an accepted Continue with a lost reply settles its exact receipt before Pause uses the new revision", async () => {
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
  const token = "429fdd84-c822-4a29-b659-d5b7fbf3ebac";
  let accepted: Command | undefined;
  let settlements = 0;
  let releaseReply!: () => void;
  const lostReply = new Promise<void>((resolve) => {
    releaseReply = resolve;
  });
  vi.stubGlobal(
    "fetch",
    vi.fn<typeof fetch>(async (path, options) => {
      if (path === "/api/v1/player/audiobooks/prepare")
        return Response.json({
          token,
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
        const body = JSON.parse(String(options?.body));
        expect(body.command).toEqual(accepted);
        settlements++;
        return Response.json({
          receipt: {
            id: accepted!.id,
            sequence: accepted!.sequence,
            revision: 8,
          },
        });
      }
      if (path === "/api/v1/player/commands") {
        const command: Command = JSON.parse(String(options?.body));
        if (command.action.kind === "continue_audiobook") {
          service.commands.push(command);
          accepted = command;
          Object.assign(service.state, {
            revision: 8,
            selection: 5,
            queue_len: 3,
            current_index: 2,
            current: {
              id: "continued-occurrence",
              recording_id: chapter.recording_id,
              chapter: {
                audiobook_id: page.detail.playlist.id,
                entry_id: chapter.entry_id,
                recording_id: chapter.recording_id,
              },
            },
            speed: 750,
          });
          service.emit("state", service.state);
          await lostReply;
          return Response.json({
            id: command.id,
            sequence: command.sequence,
            revision: 8,
          });
        }
        expect(settlements).toBe(1);
        expect(command).toMatchObject({
          sequence: 2,
          revision: 8,
          action: { kind: "pause" },
        });
      }
      return request(path, options);
    }),
  );
  service.queue = async (query) =>
    Response.json({
      revision: query.revision,
      offset: query.offset,
      total: 3,
      entries: [0, 1, 2]
        .slice(query.offset, query.offset + query.limit)
        .map((index) => ({
          id: index === 2 ? "continued-occurrence" : `continued-${index}`,
          recording: {
            id: chapter.recording_id,
            title: "Owned chapter",
            artist: "Fixture",
            duration_ms: 100000,
            available: true,
          },
          chapter: {
            audiobook_id: page.detail.playlist.id,
            entry_id: page.detail.chapters[index].entry_id,
            recording_id: chapter.recording_id,
          },
        })),
    });
  service.command = async (command) => {
    Object.assign(service.state, { playing: false, desired_playing: false });
    service.emit("state", service.state);
    return Response.json({
      id: command.id,
      sequence: command.sequence,
      revision: command.revision,
    });
  };
  const player = new NativePlayback(page.user.id);
  const controller = new AbortController();
  try {
    await player.reconnect();
    await vi.waitFor(() => expect(player.snapshot().playing).toBe(true));
    const continuing = player.continueAudiobook(
      page.detail.playlist.id,
      controller.signal,
    );
    await vi.waitFor(() => expect(accepted).toBeDefined());
    await player.setPlaying(false);
    expect(await continuing).toBe(false);
    expect(service.commands).toHaveLength(2);
    expect(service.claims).toBe(1);
    await vi.waitFor(() =>
      expect(player.snapshot().current?.id).toBe(chapter.recording_id),
    );
    expect(player.snapshot().playing).toBe(false);
    expect(player.snapshot().speed).toBe(0.75);
    expect(player.snapshot().error).toBe("");
    releaseReply();
    await Promise.resolve();
    expect(service.commands).toHaveLength(2);
    expect(player.snapshot().playing).toBe(false);
  } finally {
    releaseReply();
    controller.abort();
    player.close();
  }
});
