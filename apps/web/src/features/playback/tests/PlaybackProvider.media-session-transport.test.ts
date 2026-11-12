import { act } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { convertedHead } from "../fixtures/browserStreamMedia";
import { setStreamingQuality } from "../browser/StreamingQuality";
import { browserPreparationMedia } from "../fixtures/browserPreparationMedia";
import { browserMediaSession } from "../fixtures/browserMediaSession";
import {
  otherRecording,
  playbackRequests,
  recording,
  renderAccount,
} from "../fixtures/browserMediaSessionPlayback";

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
    expect(system.session.metadata?.album).toBe("");
    await act(async () => system.invoke("previoustrack"));
    expect(view.player().snapshot().current?.id).toBe(recording.id);
    expect(system.session.metadata?.title).toBe(recording.title);
    expect(system.session.metadata?.album).toBe(recording.album);
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

test("system controls publish the full converted timeline and seek by absolute recording time without page ticks", async () => {
  localStorage.setItem("hito:stream-quality", "saver");
  const media = browserPreparationMedia();
  const system = browserMediaSession();
  playbackRequests([recording], (path, options) =>
    options?.method === "HEAD" ? convertedHead(path) : undefined,
  );
  const view = renderAccount();
  try {
    await act(async () => {
      await view
        .player()
        .playQueue([recording], 0, { kind: "position", position: 12 });
      media.canPlay(media.created[0]);
    });
    const current = media.created[0];
    const renders = view.renders();
    act(() => {
      current.currentTime = 2;
      current.dispatchEvent(new Event("timeupdate"));
    });
    expect(system.session.position).toMatchObject({
      position: 14,
      duration: 90,
    });
    expect(view.renders()).toBe(renders);
    act(() => system.invoke("seekto", { seekTime: 40 }));
    await vi.waitFor(() => expect(current.src).toContain("start_ms=40000"));
    act(() => {
      media.canPlay(current);
      current.currentTime = 1;
      current.dispatchEvent(new Event("timeupdate"));
    });
    expect(system.session.position).toMatchObject({
      position: 41,
      duration: 90,
    });
    await act(async () => system.invoke("pause"));
    expect(system.session.playbackState).toBe("paused");
    expect(view.player().positionSnapshot().position).toBe(41);
  } finally {
    view.unmount();
    system.close();
    media.close();
  }
});

test("Strict Mode reactivates quality changes and actual unmount retires the browser owner", async () => {
  setStreamingQuality("high");
  const media = browserPreparationMedia();
  const system = browserMediaSession();
  playbackRequests([recording, otherRecording], (path, options) =>
    options?.method === "HEAD" ? convertedHead(path) : undefined,
  );
  const view = renderAccount(true);
  try {
    await act(async () => {
      await view.player().playQueue([recording, otherRecording]);
    });
    const current = document.querySelector<HTMLAudioElement>(
      "audio[data-hito-player]",
    )!;
    act(() => media.canPlay(current));
    await vi.waitFor(() => {
      expect(
        document.querySelector<HTMLAudioElement>("audio[data-hito-preload]")
          ?.src,
      ).toContain("quality=high");
    });
    const previous = document.querySelector<HTMLAudioElement>(
      "audio[data-hito-preload]",
    )!;
    const currentUrl = current.src;
    act(() => setStreamingQuality("saver"));
    await vi.waitFor(() => {
      expect(
        document.querySelector<HTMLAudioElement>("audio[data-hito-preload]")
          ?.src,
      ).toContain("quality=saver");
    });
    expect(previous.isConnected).toBe(false);
    expect(current.src).toBe(currentUrl);
    expect(current.paused).toBe(false);
    view.unmount();
    const created = media.created.length;
    setStreamingQuality("original");
    await Promise.resolve();
    expect(media.created).toHaveLength(created);
    expect(document.querySelectorAll("audio")).toHaveLength(0);
    expect(current.paused).toBe(true);
  } finally {
    view.unmount();
    system.close();
    media.close();
  }
});
