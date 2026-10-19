// @vitest-environment jsdom
import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { NoticeProvider } from "./components/Notices";
import { audiobookPages } from "./fixtures/audiobooks";
import { audiobookProgressMedia } from "./fixtures/audiobookProgressMedia";
import type { components } from "./generated";
import AudiobookPage from "./pages/AudiobookPage";
import { PlaybackProvider, usePlayer } from "./player/PlaybackProvider";
import type { Player } from "./player/model";
import { nativeService } from "./player/native/fixtures/service";

type Schema = components["schemas"];
vi.mock("@inertiajs/react", () => ({
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test.each([
  { mode: "browser", outcome: "commit" },
  { mode: "native", outcome: "commit" },
  { mode: "browser", outcome: "timeout" },
  { mode: "native", outcome: "timeout" },
] as const)(
  "$mode Continue handles $outcome through its concrete player without trusting rendered progress",
  async ({ mode, outcome }) => {
    vi.spyOn(console, "info").mockImplementation(() => {});
    vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.spyOn(console, "debug").mockImplementation(() => {});
    const { detail: page } = audiobookPages();
    const chapters = page.detail.chapters.slice(0, 3);
    const anchor = (index: number) => ({
      audiobook_id: page.detail.playlist.id,
      entry_id: chapters[index].entry_id,
      recording_id: chapters[index].recording_id!,
    });
    page.detail.continuation = {
      chapter: anchor(0),
      progress: {
        recording_id: chapters[0].recording_id!,
        position_ms: 1000,
        speed: 1.25,
      },
    };
    const fresh: Schema["AudiobookPlayback"] = {
      audiobook_id: page.detail.playlist.id,
      chapter: anchor(2),
      progress: {
        recording_id: chapters[2].recording_id!,
        position_ms: 35125,
        speed: 0.75,
      },
      chapters,
    };
    const token = "429fdd84-c822-4a29-b659-d5b7fbf3ebac";
    const media = mode === "browser" ? audiobookProgressMedia() : null;
    const native = mode === "native" ? nativeService() : null;
    const nativeRequest = native
      ? vi.mocked(fetch).getMockImplementation()!
      : null;
    let committed = false;
    const nativeEntries = chapters.map((entry, index) => ({
      id: `native-chapter-${index}`,
      recording: { ...entry, id: entry.recording_id! },
      chapter: anchor(index),
    }));
    if (native) {
      Object.assign(native.state, {
        revision: 7,
        selection: 4,
        queue_len: 1,
        current_index: 0,
        current: {
          id: "old-native-entry",
          recording_id: chapters[1].recording_id,
          chapter: anchor(1),
        },
        phase: "ready",
        desired_playing: true,
        playing: true,
        speed: 1500,
      });
      native.queue = async (query) =>
        Response.json({
          revision: query.revision,
          total: committed ? 3 : 1,
          offset: query.offset,
          entries: committed
            ? nativeEntries.slice(query.offset, query.offset + query.limit)
            : [
                {
                  id: "old-native-entry",
                  recording: { ...chapters[1], id: chapters[1].recording_id },
                  chapter: anchor(1),
                },
              ],
        });
      native.command = async (command) => {
        expect(command.action).toEqual({ kind: "continue_audiobook", token });
        committed = true;
        Object.assign(native.state, {
          revision: 8,
          selection: 5,
          queue_len: 3,
          current_index: 2,
          current: {
            id: nativeEntries[2].id,
            recording_id: fresh.chapter.recording_id,
            chapter: fresh.chapter,
          },
          speed: 750,
          playing: false,
        });
        native.emit("state", native.state);
        native.emit("position", {
          selection: 5,
          entry: nativeEntries[2].id,
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
    }
    let release!: () => void;
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    const preparations: { path: string; body: unknown; signal: AbortSignal }[] =
      [];
    const requests: string[] = [];
    let cancellations = 0;
    vi.stubGlobal(
      "fetch",
      vi.fn<typeof globalThis.fetch>(async (input, options) => {
        const path = String(input);
        requests.push(path);
        expect(options?.credentials).toBe("same-origin");
        expect(new Headers(options?.headers).get("x-hito-account")).toBe(
          page.user.id,
        );
        const prepare =
          mode === "native"
            ? "/api/v1/player/audiobooks/prepare"
            : `/api/v1/audiobooks/${page.detail.playlist.id}/playback`;
        if (path === prepare) {
          preparations.push({
            path,
            body:
              options?.body == null ? null : JSON.parse(String(options.body)),
            signal: options?.signal as AbortSignal,
          });
          await held;
          return Response.json(
            mode === "native"
              ? {
                  token,
                  chapter: fresh.chapter,
                  position_ms: fresh.progress.position_ms,
                  speed: 750,
                  count: 3,
                }
              : fresh,
          );
        }
        if (nativeRequest && path === "/api/v1/player/audiobooks/cancel") {
          cancellations++;
          expect(JSON.parse(String(options?.body))).toEqual({
            lease: "lease-1",
            id: (preparations[0].body as Schema["PlayerAudiobookRequest"]).id,
          });
          return new Response(null, { status: 204 });
        }
        if (nativeRequest) return nativeRequest(input, options);
        if (path === "/api/v1/sync") {
          const batch: Schema["SyncBatch"] = JSON.parse(String(options?.body));
          return Response.json({
            acknowledged: batch.operations.map(({ id }) => id),
          });
        }
        const index = chapters.findIndex(
          (entry) =>
            path ===
            `/api/v1/recordings/${entry.recording_id}/playback-sources`,
        );
        if (index < 0)
          throw new Error("Unexpected owned Continue page request");
        return Response.json([
          {
            id:
              index === 1
                ? "189b7475-24a0-4863-bb66-1492067a3fa3"
                : "47f373bb-4296-4e89-bd3c-5b3e73833ac5",
            recording_id: chapters[index].recording_id!,
            provider: "plugin",
            device_id: null,
            ready: true,
            can_prepare: false,
            label: "Owned fixture source",
          } satisfies Schema["Source"],
        ]);
      }),
    );
    let player!: Player;
    function Page() {
      player = usePlayer();
      return createElement(AudiobookPage, page);
    }
    const view = render(
      createElement(
        NoticeProvider,
        null,
        createElement(PlaybackProvider, {
          account: page.user.id,
          native: mode === "native",
          children: createElement(Page),
        }),
      ),
    );
    try {
      if (media) {
        await act(async () => {
          player.speed(1.5);
          await player.playQueue([
            {
              recording: { ...chapters[1], id: chapters[1].recording_id! },
              chapter: anchor(1),
            },
          ]);
          media.ready(media.current());
          media.observePlaying(media.current());
        });
      } else
        await waitFor(() =>
          expect(player.snapshot().current?.id).toBe(chapters[1].recording_id),
        );
      const before = player.queueSnapshot();
      const button = screen.getByRole("button", {
        name: /^Continue audiobook$/,
      });
      expect(preparations).toEqual([]);
      if (outcome === "timeout") {
        vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
        await act(async () => {
          fireEvent.click(button);
          await vi.advanceTimersByTimeAsync(0);
        });
        expect(preparations).toHaveLength(1);
      } else {
        fireEvent.click(button);
        await waitFor(() => expect(preparations).toHaveLength(1));
      }
      expect(button).toHaveProperty("disabled", true);
      expect(player.queueSnapshot()).toEqual(before);
      expect(player.snapshot().current?.id).toBe(chapters[1].recording_id);
      expect(player.snapshot().speed).toBe(1.5);
      expect(player.snapshot().playing).toBe(true);
      expect(preparations[0].signal.aborted).toBe(false);
      if (mode === "native") {
        expect(native!.commands).toEqual([]);
        expect(preparations[0].body).toEqual({
          id: expect.any(String),
          lease: "lease-1",
          revision: 7,
          selection: 4,
          audiobook_id: page.detail.playlist.id,
        });
      } else expect(preparations[0].body).toBeNull();
      if (outcome === "timeout") {
        await act(async () => vi.advanceTimersByTimeAsync(9999));
        expect(screen.queryByRole("alert")).toBeNull();
        expect(preparations[0].signal.aborted).toBe(false);
        await act(async () => vi.advanceTimersByTimeAsync(1));
        expect(button).toHaveProperty("disabled", false);
        expect(preparations[0].signal.aborted).toBe(true);
        expect(screen.getByRole("alert").textContent).toMatch(
          /continue|timed out/i,
        );
        expect(player.queueSnapshot()).toEqual(before);
        expect(player.snapshot().current?.id).toBe(chapters[1].recording_id);
        expect(player.snapshot().speed).toBe(1.5);
        expect(player.snapshot().playing).toBe(true);
        expect(cancellations).toBe(mode === "native" ? 1 : 0);
        await act(async () => {
          release();
          await vi.advanceTimersByTimeAsync(0);
        });
        expect(player.queueSnapshot()).toEqual(before);
        expect(player.snapshot().current?.id).toBe(chapters[1].recording_id);
        expect(player.snapshot().speed).toBe(1.5);
        expect(player.snapshot().playing).toBe(true);
        if (native) expect(native.commands).toEqual([]);
        return;
      }
      await act(async () => release());
      await waitFor(() => expect(player.queueSnapshot().currentIndex).toBe(2));
      await waitFor(() =>
        expect(player.snapshot().current?.id).toBe(fresh.chapter.recording_id),
      );
      const queue = await player.queuePage(0, 10);
      expect(queue.entries[2].chapter).toEqual(fresh.chapter);
      expect(queue.entries[0].recording.id).toBe(queue.entries[2].recording.id);
      expect(queue.entries[0].id).not.toBe(queue.entries[2].id);
      expect(player.snapshot().speed).toBe(0.75);
      expect(player.positionSnapshot().position).toBe(35.125);
      await waitFor(() => expect(button).toHaveProperty("disabled", false));
      expect(preparations).toHaveLength(1);
      expect(screen.queryByRole("alert")).toBeNull();
      if (native) {
        expect(native.commands).toHaveLength(1);
        expect(
          requests.every((path) => path.startsWith("/api/v1/player")),
        ).toBe(true);
        expect(document.querySelectorAll("audio")).toHaveLength(0);
      } else
        expect(
          requests.every(
            (path) =>
              path === "/api/v1/sync" ||
              path ===
                `/api/v1/audiobooks/${page.detail.playlist.id}/playback` ||
              /\/playback-sources$/.test(path),
          ),
        ).toBe(true);
    } finally {
      release();
      view.unmount();
    }
  },
);
