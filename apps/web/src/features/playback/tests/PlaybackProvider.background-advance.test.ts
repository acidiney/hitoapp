import { act } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import {
  audioPath,
  browserPreparationMedia,
} from "../fixtures/browserPreparationMedia";
import { browserMediaSession } from "../fixtures/browserMediaSession";
import {
  otherRecording,
  playbackRequests,
  recording,
  renderAccount,
} from "../fixtures/browserMediaSessionPlayback";

const firstAudio = audioPath("49858203-ce08-478f-95fe-482f34e376fb");
const secondAudio = audioPath(otherRecording.id);

function pageIs(state: DocumentVisibilityState) {
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    get: () => state,
  });
}

beforeEach(() => {
  vi.useFakeTimers({
    toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval"],
  });
});

afterEach(() => {
  vi.useRealTimers();
  Reflect.deleteProperty(document, "visibilityState");
});

async function playFirstWithBufferedSuccessor() {
  const media = browserPreparationMedia();
  const system = browserMediaSession();
  const requests = playbackRequests([recording, otherRecording], (path) =>
    path === "/health" ? new Response("ok") : undefined,
  );
  const view = renderAccount();
  await act(async () => {
    await view.player().playQueue([recording, otherRecording]);
  });
  const player = media.created[0];
  const warm = await media.loadedSource(otherRecording.id);
  await act(async () => media.canPlay(warm));
  return { media, system, requests, view, player, warm };
}

test("a locked page advances on the buffered element with play called inside the ended handler and no timers", async () => {
  pageIs("hidden");
  const { media, system, requests, view, player, warm } =
    await playFirstWithBufferedSuccessor();
  try {
    expect(media.played.map((play) => play.source)).toEqual([firstAudio]);
    expect(document.querySelectorAll("audio[data-hito-player]")).toHaveLength(
      1,
    );
    const requestsBefore = requests.length;

    act(() => {
      media.end(player);
      expect(media.played.at(-1)).toEqual({ media: warm, source: secondAudio });
      expect(view.player().snapshot().current?.id).toBe(otherRecording.id);
    });

    expect(media.played.map((play) => play.media)).toEqual([player, warm]);
    expect(media.loaded.filter((load) => load.source === secondAudio)).toEqual([
      { media: warm, source: secondAudio },
    ]);
    expect(player.paused).toBe(true);
    expect(player.isConnected).toBe(false);
    expect(document.querySelectorAll("audio[data-hito-player]")).toHaveLength(
      1,
    );
    expect(document.querySelector("audio[data-hito-player]")).toBe(warm);
    expect(
      requests
        .slice(requestsBefore)
        .filter((request) => request.path !== "/api/v1/sync"),
    ).toEqual([]);
    expect(system.session.metadata).toMatchObject({
      title: otherRecording.title,
      artist: otherRecording.artist,
      album: "",
    });
    expect(system.session.playbackState).toBe("playing");
    act(() => {
      warm.currentTime = 12;
      warm.dispatchEvent(new Event("timeupdate"));
    });
    expect(system.session.position).toEqual({
      duration: 90,
      position: 12,
      playbackRate: 1,
    });
    expect(view.player().snapshot().playing).toBe(true);
  } finally {
    view.unmount();
    system.close();
    media.close();
  }
});

for (const visibility of ["hidden", "visible"] as const)
  test(`a ${visibility} page that stalls shows Buffering, stops the system clock and reopens the same element at the same position after fifteen seconds`, async () => {
    pageIs(visibility);
    const { media, system, requests, view, player } =
      await playFirstWithBufferedSuccessor();
    try {
      act(() => {
        player.currentTime = 30;
        player.dispatchEvent(new Event("timeupdate"));
      });
      const requestsBefore = requests.length;
      const playsBefore = media.played.length;

      act(() => {
        player.dispatchEvent(new Event("waiting"));
      });
      expect(view.player().snapshot()).toMatchObject({
        buffering: true,
        playing: true,
      });
      expect(system.session.playbackState).toBe("paused");
      expect(system.session.position).toMatchObject({ position: 30 });

      await act(async () => {
        await vi.advanceTimersByTimeAsync(14_999);
      });
      expect(media.played).toHaveLength(playsBefore);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(1);
      });
      expect(media.played).toHaveLength(playsBefore + 1);
      expect(media.played.at(-1)).toMatchObject({
        media: player,
        source: firstAudio,
      });
      expect(document.querySelector("audio[data-hito-player]")).toBe(player);
      act(() => media.canPlay(player));
      expect(player.currentTime).toBe(30);
      expect(
        requests
          .slice(requestsBefore)
          .filter((request) => request.path !== "/api/v1/sync"),
      ).toEqual([{ path: "/health", method: "GET", account: null }]);

      act(() => {
        player.dispatchEvent(new Event("playing"));
      });
      expect(view.player().snapshot()).toMatchObject({
        buffering: false,
        playing: true,
      });
      expect(system.session.playbackState).toBe("playing");
    } finally {
      view.unmount();
      system.close();
      media.close();
    }
  });
