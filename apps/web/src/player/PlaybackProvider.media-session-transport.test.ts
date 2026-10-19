// @vitest-environment jsdom
import { act, cleanup } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { browserPreparationMedia } from "./fixtures/browserPreparationMedia";
import { browserMediaSession } from "./fixtures/browserMediaSession";
import {
  otherRecording,
  playbackRequests,
  recording,
  renderAccount,
} from "./fixtures/browserMediaSessionPlayback";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("system relative seeks use valid positive offsets and remain inside the current recording", async () => {
  const media = browserPreparationMedia();
  const system = browserMediaSession();
  const requests = playbackRequests();
  const view = renderAccount();
  try {
    await act(async () => {
      await view.player().playQueue([recording]);
      media.canPlay(media.created[0]);
    });
    act(() => system.invoke("seekto", { seekTime: 85 }));
    act(() => system.invoke("seekforward", { seekOffset: 30 }));
    expect(view.player().positionSnapshot().position).toBe(90);
    act(() => system.invoke("seekbackward", { seekOffset: 500 }));
    expect(view.player().positionSnapshot().position).toBe(0);
    act(() => system.invoke("seekforward"));
    expect(view.player().positionSnapshot().position).toBe(10);
    act(() => {
      for (const seekOffset of [-2, 0, Number.NaN, Number.POSITIVE_INFINITY]) {
        system.invoke("seekforward", { seekOffset });
        system.invoke("seekbackward", { seekOffset });
      }
    });
    expect(view.player().positionSnapshot().position).toBe(10);
    expect(system.session.position?.position).toBe(10);
    expect(requests).toHaveLength(1);
  } finally {
    view.unmount();
    system.close();
    media.close();
  }
});

test("system next, previous, and stop follow the owned queue and never restart after stop", async () => {
  const media = browserPreparationMedia();
  const system = browserMediaSession();
  const requests = playbackRequests([recording, otherRecording]);
  const view = renderAccount();
  try {
    await act(async () => {
      await view.player().playQueue([recording, otherRecording]);
    });
    const prepared = await media.loadedSource(otherRecording.id);
    act(() => media.canPlay(prepared));
    await act(async () => system.invoke("nexttrack"));
    expect(view.player().snapshot().current?.id).toBe(otherRecording.id);
    expect(system.session.metadata?.title).toBe(otherRecording.title);
    await act(async () => system.invoke("previoustrack"));
    expect(view.player().snapshot().current?.id).toBe(recording.id);
    expect(system.session.metadata?.title).toBe(recording.title);
    const oldCallbacks = [...system.handlers];
    act(() => system.invoke("stop"));
    expect(view.player().snapshot().current).toBeNull();
    expect(view.player().queueSnapshot().total).toBe(0);
    expect(system.session.metadata).toBeNull();
    expect(system.session.playbackState).toBe("none");
    expect(system.session.position).toBeUndefined();
    expect(system.handlers.size).toBe(0);
    const requestCount = requests.length;
    const plays = media.played.length;
    await act(async () => {
      for (const [action, callback] of oldCallbacks)
        callback({ action, seekTime: 20 });
    });
    expect(view.player().snapshot().current).toBeNull();
    expect(requests).toHaveLength(requestCount);
    expect(media.played).toHaveLength(plays);
    expect(
      requests.every(
        (request) =>
          request.path.endsWith("/playback-sources") ||
          request.path === "/api/v1/sync",
      ),
    ).toBe(true);
  } finally {
    view.unmount();
    system.close();
    media.close();
  }
});
