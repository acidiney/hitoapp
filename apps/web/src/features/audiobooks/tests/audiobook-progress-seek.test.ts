import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { audiobookPages } from "../fixtures/audiobooks";
import AudiobookPage from "../pages/AudiobookPage";
import {
  PlaybackProvider,
  usePlayer,
} from "../../playback/components/PlaybackProvider";
import type { Player } from "../../playback/model";

vi.mock("@inertiajs/react", () => ({
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

function seekingMedia() {
  type State = {
    paused: boolean;
    seeking: boolean;
    ready: number;
    position: number;
  };
  const states = new WeakMap<HTMLAudioElement, State>();
  vi.stubGlobal("Audio", function () {
    const media = document.createElement("audio");
    const state: State = {
      paused: true,
      seeking: false,
      ready: 0,
      position: 0,
    };
    states.set(media, state);
    Object.defineProperties(media, {
      paused: { get: () => state.paused },
      ended: { get: () => false },
      readyState: { get: () => state.ready },
      duration: { get: () => 120 },
      seeking: { get: () => state.seeking },
      currentSrc: { get: () => media.src },
      currentTime: {
        get: () => state.position,
        set: (position: number) => {
          if (position !== state.position) state.seeking = true;
          state.position = position;
        },
      },
      play: {
        value: async () => {
          state.paused = false;
          media.dispatchEvent(new Event("play"));
        },
      },
      pause: {
        value: () => {
          const changed = !state.paused;
          state.paused = true;
          if (changed) media.dispatchEvent(new Event("pause"));
        },
      },
      load: {
        value: () => {
          state.position = 0;
          state.ready = 0;
          state.seeking = false;
          media.dispatchEvent(new Event("loadstart"));
        },
      },
    });
    return media;
  });
  return {
    current() {
      const media = document.querySelector<HTMLAudioElement>(
        "audio[data-hito-player]",
      );
      if (!media) throw new Error("The account has no active audio element");
      return media;
    },
    ready(media: HTMLAudioElement) {
      states.get(media)!.ready = 1;
      media.dispatchEvent(new Event("loadedmetadata"));
      states.get(media)!.ready = 4;
      media.dispatchEvent(new Event("canplay"));
    },
    observe(media: HTMLAudioElement, position: number) {
      states.get(media)!.position = position;
      media.dispatchEvent(new Event("playing"));
      media.dispatchEvent(new Event("timeupdate"));
    },
    completeSeek(media: HTMLAudioElement, position: number) {
      const state = states.get(media)!;
      state.position = position;
      state.seeking = false;
      media.dispatchEvent(new Event("seeked"));
    },
  };
}

test("a completed seek can checkpoint its heard chapter after the media clock advances", async () => {
  vi.spyOn(console, "info").mockImplementation(() => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.spyOn(console, "debug").mockImplementation(() => {});
  const client = "c1607463-f8a9-4b46-96b6-20b424c568ac";
  const previousClient = localStorage.getItem("hito-device");
  localStorage.setItem("hito-device", client);
  const media = seekingMedia();
  const { detail: page } = audiobookPages();
  const chapter = page.detail.chapters[2];
  const source: Schema<"Source"> = {
    id: "96915255-c05e-40b8-bdfd-6389049aabf7",
    recording_id: chapter.recording_id!,
    provider: "local",
    device_id: "14d36a04-cd07-44d9-8f2c-4a5981b6a25e",
    ready: true,
    label: "Owned chapter audio",
    can_prepare: false,
    match_kind: "verified",
  };
  const submitted: Schema<"SyncBatch">[] = [];
  const unexpected: string[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, options?: RequestInit) => {
      const path = String(input);
      const method = options?.method ?? "GET";
      expect(new Headers(options?.headers).get("x-hito-account")).toBe(
        page.user.id,
      );
      expect(options?.credentials).toBe("same-origin");
      expect(options?.signal).toBeInstanceOf(AbortSignal);
      if (
        path === `/api/v1/audiobooks/${page.detail.playlist.id}` &&
        method === "GET"
      )
        return Response.json(page.detail);
      if (
        path === `/api/v1/recordings/${source.recording_id}/playback-sources` &&
        method === "GET"
      )
        return Response.json([source]);
      if (path === "/api/v1/sync" && method === "POST") {
        const batch = JSON.parse(String(options?.body)) as Schema<"SyncBatch">;
        submitted.push(batch);
        return Response.json({
          acknowledged: batch.operations.map(({ id }) => id),
        });
      }
      unexpected.push(`${method} ${path}`);
      throw new Error("Chapter seeking requested an unowned operation");
    }),
  );
  let player!: Player;
  function Capture() {
    player = usePlayer();
    return createElement(AudiobookPage, page);
  }
  const view = render(
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account: page.user.id,
        children: createElement(Capture),
      }),
    ),
  );
  const checkpoint = (position_ms: number): Schema<"Action">[] => [
    {
      kind: "progress_set",
      recording_id: source.recording_id,
      position_ms,
      speed: 1,
    },
    {
      kind: "audiobook_chapter_set",
      chapter: {
        audiobook_id: page.detail.playlist.id,
        entry_id: chapter.entry_id,
        recording_id: source.recording_id,
      },
    },
  ];
  try {
    fireEvent.click(
      screen.getByRole("button", { name: "Play chapter 3 from beginning" }),
    );
    await waitFor(() => expect(player.snapshot().playing).toBe(true));
    const audio = media.current();
    expect(audio.getAttribute("src")).toBe(
      `/api/v1/sources/${source.id}/audio`,
    );
    expect(submitted).toEqual([]);
    await act(async () => {
      media.ready(audio);
      media.observe(audio, 0);
    });
    await waitFor(() => expect(submitted).toHaveLength(1));
    expect(submitted[0].operations.map(({ action }) => action)).toEqual(
      checkpoint(0),
    );
    const queue = await player.queuePage(0, 10);
    expect(player.queueSnapshot().currentEntry).toBe(queue.entries[2].id);
    expect(queue.entries[2].chapter?.entry_id).toBe(chapter.entry_id);
    expect(queue.entries[0].recording.id).toBe(queue.entries[2].recording.id);
    expect(queue.entries[0].id).not.toBe(queue.entries[2].id);

    await act(async () => {
      player.seek(35);
      expect(audio.seeking).toBe(true);
      media.observe(audio, 35.01);
      audio.dispatchEvent(new Event("seeked"));
    });
    expect(submitted).toHaveLength(1);

    await act(async () => media.completeSeek(audio, 35.01));
    await waitFor(() => expect(submitted).toHaveLength(2));
    expect(submitted[1].operations.map(({ action }) => action)).toEqual(
      checkpoint(35_010),
    );
    await act(async () => media.observe(audio, 45.02));
    await waitFor(() => expect(submitted).toHaveLength(3));
    expect(submitted[2].operations.map(({ action }) => action)).toEqual(
      checkpoint(45_020),
    );
    expect(submitted.every(({ device_id }) => device_id === client)).toBe(true);
    const ids = submitted.flatMap(({ operations }) =>
      operations.map(({ id }) => id),
    );
    expect(new Set(ids).size).toBe(6);
    expect(player.queueSnapshot().currentEntry).toBe(queue.entries[2].id);
    expect(unexpected).toEqual([]);
  } finally {
    view.unmount();
    if (previousClient === null) localStorage.removeItem("hito-device");
    else localStorage.setItem("hito-device", previousClient);
    expect(document.querySelectorAll("audio")).toHaveLength(0);
  }
});
