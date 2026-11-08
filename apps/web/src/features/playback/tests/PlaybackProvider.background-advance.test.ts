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
  const requests = playbackRequests([recording, otherRecording]);
  const view = renderAccount();
  await act(async () => {
    await view.player().playQueue([recording, otherRecording]);
  });
  const player = media.created[0];
  const warm = await media.loadedSource(otherRecording.id);
  await act(async () => media.canPlay(warm));
  return { media, system, requests, view, player, warm };
}

test("a locked page advances on the one playing element with play called inside the ended handler and no timers", async () => {
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
      expect(player.getAttribute("src")).toBe(secondAudio);
      expect(media.played.map((play) => play.source)).toEqual([
        firstAudio,
        secondAudio,
      ]);
      expect(view.player().snapshot().current?.id).toBe(otherRecording.id);
    });

    expect(media.played.every((play) => play.media === player)).toBe(true);
    expect(warm.paused).toBe(true);
    expect(warm.isConnected).toBe(false);
    expect(document.querySelectorAll("audio[data-hito-player]")).toHaveLength(
      1,
    );
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
      media.canPlay(player);
      player.currentTime = 12;
      player.dispatchEvent(new Event("timeupdate"));
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

test("a hidden page retries a stalled track once on the same element at the same position and then falls back to source recovery", async () => {
  pageIs("hidden");
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
      player.dispatchEvent(new Event("stalled"));
    });
    expect(media.played).toHaveLength(playsBefore + 1);
    expect(media.played.at(-1)).toMatchObject({
      media: player,
      source: firstAudio,
    });
    expect(view.player().snapshot().loading).toBe(false);
    act(() => media.canPlay(player));
    expect(player.currentTime).toBe(30);
    expect(requests).toHaveLength(requestsBefore);

    act(() => {
      player.dispatchEvent(new Event("stalled"));
    });
    expect(media.played).toHaveLength(playsBefore + 1);

    act(() => {
      player.dispatchEvent(new Event("error"));
    });
    expect(media.played).toHaveLength(playsBefore + 1);
    expect(view.player().snapshot().loading).toBe(true);
  } finally {
    view.unmount();
    system.close();
    media.close();
  }
});

test("a visible page leaves a stall to the browser", async () => {
  pageIs("visible");
  const { media, system, view, player } =
    await playFirstWithBufferedSuccessor();
  try {
    act(() => {
      media.canPlay(player);
      player.currentTime = 30;
      player.dispatchEvent(new Event("timeupdate"));
    });
    const playsBefore = media.played.length;
    act(() => {
      player.dispatchEvent(new Event("stalled"));
    });
    expect(media.played).toHaveLength(playsBefore);
  } finally {
    view.unmount();
    system.close();
    media.close();
  }
});
