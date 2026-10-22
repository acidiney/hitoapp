import { expect, test, vi } from "vitest";
import { audiobookPages } from "../../../audiobooks/fixtures/audiobooks";
import { NativePlayback } from "../../native/NativePlayback";
import { nativeService } from "../../fixtures/native/service";
import type { Command } from "../../native/api";

test("navigation cancellation permits an immediate Continue retry on the same native session", async () => {
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
  const preparations: { id: string; revision: number; selection: number }[] =
    [];
  const tokens = [
    "429fdd84-c822-4a29-b659-d5b7fbf3ebac",
    "d540289a-d598-4e40-a20e-cf66d4a786c3",
  ];
  let releaseFirst!: () => void;
  const firstReply = new Promise<void>((resolve) => {
    releaseFirst = resolve;
  });
  let cancellations = 0;
  vi.stubGlobal(
    "fetch",
    vi.fn<typeof fetch>(async (path, options) => {
      if (path === "/api/v1/player/audiobooks/prepare") {
        preparations.push(JSON.parse(String(options?.body)));
        return Response.json({
          token: tokens[preparations.length - 1],
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
        const input = JSON.parse(String(options?.body));
        expect(input.command).toEqual(service.commands[0]);
        cancellations++;
        return Response.json({ receipt: null });
      }
      if (path === "/api/v1/player/commands") {
        const command: Command = JSON.parse(String(options?.body));
        service.commands.push(command);
        expect(command.action.kind).toBe("continue_audiobook");
        if (service.commands.length === 1) {
          await firstReply;
          return Response.json(
            { message: "The candidate was retired" },
            { status: 409 },
          );
        }
        expect(command).toMatchObject({
          sequence: 2,
          revision: 7,
          action: { kind: "continue_audiobook", token: tokens[1] },
        });
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
  const navigation = new AbortController();
  const retry = new AbortController();
  try {
    await player.reconnect();
    await vi.waitFor(() => expect(player.snapshot().playing).toBe(true));
    const before = player.queueSnapshot();
    const first = player.continueAudiobook(
      page.detail.playlist.id,
      navigation.signal,
    );
    await vi.waitFor(() => expect(service.commands).toHaveLength(1));
    navigation.abort();
    expect(await first).toBe(false);
    expect(cancellations).toBe(1);
    expect(player.queueSnapshot()).toEqual(before);
    expect(player.snapshot().playing).toBe(true);
    expect(player.snapshot().speed).toBe(1.5);
    expect(player.snapshot().error).toBe("");
    const second = player.continueAudiobook(
      page.detail.playlist.id,
      retry.signal,
    );
    await vi.waitFor(() => expect(preparations).toHaveLength(2));
    expect(await second).toBe(true);
    expect(preparations[1]).toMatchObject({ revision: 7, selection: 4 });
    expect(preparations[1].id).not.toBe(preparations[0].id);
    expect(service.commands).toHaveLength(2);
    expect(service.commands[1].id).not.toBe(service.commands[0].id);
    expect(service.claims).toBe(1);
    releaseFirst();
    await Promise.resolve();
    expect(service.commands).toHaveLength(2);
    expect(player.snapshot().error).toBe("");
  } finally {
    releaseFirst();
    navigation.abort();
    retry.abort();
    player.close();
  }
});
