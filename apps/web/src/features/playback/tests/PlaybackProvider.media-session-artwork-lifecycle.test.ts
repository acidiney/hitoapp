import { act } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { browserPreparationMedia } from "../fixtures/browserPreparationMedia";
import { browserMediaSession } from "../fixtures/browserMediaSession";
import { browserSessionImages } from "../fixtures/browserSessionImages";
import {
  otherRecording,
  playbackRequests,
  recording,
  renderAccount,
} from "../fixtures/browserMediaSessionPlayback";

test("held image loads cannot publish after track replacement, account replacement, or queue clearing", async () => {
  const media = browserPreparationMedia();
  const system = browserMediaSession();
  const images = browserSessionImages();
  const requests = playbackRequests([recording, otherRecording]);
  const view = renderAccount();
  try {
    await act(async () => {
      await view.player().playQueue([recording]);
    });
    images.size(images.created[0]);
    const firstLoad = images.holdLoad(images.created[0]);
    await act(async () => {
      await view.player().playQueue([otherRecording]);
    });
    expect(images.created).toHaveLength(2);
    expect(images.created[0].getAttribute("src")).toBeNull();
    act(firstLoad);
    expect(system.session.metadata?.title).toBe(otherRecording.title);
    expect(system.session.metadata?.artwork).toEqual([]);

    images.size(images.created[1]);
    const secondLoad = images.holdLoad(images.created[1]);
    view.rerender(view.accountView("second-owned-account"));
    expect(system.session.metadata).toBeNull();
    await act(async () => {
      await view.player().playQueue([recording]);
    });
    expect(images.created).toHaveLength(3);
    act(() => {
      firstLoad();
      secondLoad();
    });
    expect(system.session.metadata?.title).toBe(recording.title);
    expect(system.session.metadata?.artwork).toEqual([]);
    images.size(images.created[2]);
    const thirdLoad = images.holdLoad(images.created[2]);
    act(() => view.player().clear());
    act(thirdLoad);
    expect(system.session.metadata).toBeNull();
    expect(
      images.created.every((image) => image.getAttribute("src") === null),
    ).toBe(true);
    expect(requests).toHaveLength(3);
    expect(
      requests.every(
        (request) =>
          request.method === "GET" &&
          request.path.endsWith("/playback-sources"),
      ),
    ).toBe(true);
  } finally {
    view.unmount();
    system.close();
    media.close();
  }
});

test.each(["error", "timeout"] as const)(
  "an artwork %s ends its bounded probe without changing playback",
  async (outcome) => {
    vi.useFakeTimers();
    const media = browserPreparationMedia();
    const system = browserMediaSession();
    const images = browserSessionImages();
    const requests = playbackRequests();
    const view = renderAccount();
    try {
      await act(async () => {
        await view.player().playQueue([recording]);
      });
      const playingMedia = media.created[0];
      expect(playingMedia.getAttribute("src")).not.toBeNull();
      act(() => {
        playingMedia.currentTime = 1;
        playingMedia.dispatchEvent(new Event("timeupdate"));
      });
      expect(view.player().snapshot().desiredPlaying).toBeUndefined();
      const probe = images.created[0];
      images.size(probe);
      const lateLoad = images.holdLoad(probe);
      if (outcome === "error")
        act(() => probe.dispatchEvent(new Event("error")));
      else
        await act(async () => {
          await vi.advanceTimersByTimeAsync(10_000);
        });
      expect(probe.getAttribute("src")).toBeNull();
      expect(system.session.metadata?.artwork).toEqual([]);
      act(lateLoad);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(60_000);
      });
      expect(images.created).toHaveLength(1);
      expect(system.session.metadata?.artwork).toEqual([]);
      expect(view.player().snapshot().playing).toBe(true);
      expect(requests).toHaveLength(1);
    } finally {
      view.unmount();
      system.close();
      media.close();
    }
  },
);
