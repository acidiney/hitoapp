// @vitest-environment jsdom
import { act, cleanup } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { browserPreparationMedia } from "./fixtures/browserPreparationMedia";
import { browserMediaSession } from "./fixtures/browserMediaSession";
import { browserSessionImages } from "./fixtures/browserSessionImages";
import {
  playbackRequests,
  recording,
  renderAccount,
} from "./fixtures/browserMediaSessionPlayback";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("system artwork is the authorized current recording image only after the browser loads it", async () => {
  const media = browserPreparationMedia();
  const system = browserMediaSession();
  const images = browserSessionImages();
  const requests = playbackRequests();
  const view = renderAccount();
  try {
    await act(async () => {
      await view.player().playQueue([recording]);
    });
    expect(system.session.metadata?.artwork).toEqual([]);
    expect(images.created).toHaveLength(1);
    const probe = images.created[0];
    const path = `/api/v1/recordings/${recording.id}/artwork`;
    const url = new URL(path, location.href);
    expect(new URL(probe.src)).toEqual(url);
    expect(new URL(probe.src).origin).toBe(location.origin);
    expect(new URL(probe.src).search).toBe("");
    act(() => images.load(probe, 640, 480));
    expect(system.session.metadata).toMatchObject({
      title: recording.title,
      artist: recording.artist,
      artwork: [{ src: url.href, sizes: "640x480" }],
    });
    act(() =>
      view
        .player()
        .setRecordingField(
          recording.id,
          "title",
          "Personal private title",
          recording.title,
        ),
    );
    expect(system.session.metadata?.title).toBe("Personal private title");
    expect(system.session.metadata?.artwork).toEqual([
      { src: url.href, sizes: "640x480" },
    ]);
    expect(images.created).toHaveLength(1);
    expect(requests).toEqual([
      {
        path: `/api/v1/recordings/${recording.id}/sources`,
        method: "GET",
        account: "owned-system-account",
      },
    ]);
    view.unmount();
    expect(system.session.metadata).toBeNull();
    expect(probe.getAttribute("src")).toBeNull();
  } finally {
    view.unmount();
    system.close();
    media.close();
  }
});
