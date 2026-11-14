import type { Schema } from "../../../shared/schema";
import { act } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { browserPreparationMedia } from "../fixtures/browserPreparationMedia";
import { browserMediaSession } from "../fixtures/browserMediaSession";
import {
  playbackRequests,
  recording,
  renderAccount,
} from "../fixtures/browserMediaSessionPlayback";

test("repeated system play during a pending browser play does not toggle playback and an opposite pause cancels its intent", async () => {
  const media = browserPreparationMedia();
  const system = browserMediaSession();
  const requests = playbackRequests();
  const view = renderAccount();
  let held: ReturnType<typeof media.holdNextPlay> | undefined;
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
    expect(view.player().snapshot().playing).toBe(true);
    await act(async () => system.invoke("pause"));
    expect(view.player().snapshot().playing).toBe(false);
    held = media.holdNextPlay();
    act(() => system.invoke("play"));
    const audio = await held.entered;
    expect(audio.paused).toBe(false);
    expect(view.player().snapshot().playing).toBe(false);
    const pauses = vi.mocked(HTMLMediaElement.prototype.pause).mock.calls
      .length;
    const plays = media.played.length;
    act(() => {
      system.invoke("play");
      system.invoke("play");
    });
    expect(HTMLMediaElement.prototype.pause).toHaveBeenCalledTimes(pauses);
    expect(media.played).toHaveLength(plays);
    expect(audio.paused).toBe(false);
    act(() => {
      system.invoke("pause");
      system.invoke("pause");
    });
    expect(audio.paused).toBe(true);
    expect(view.player().snapshot().playing).toBe(false);
    expect(system.session.playbackState).toBe("paused");
    expect(media.played).toHaveLength(plays);
    await act(async () => held?.release());
    expect(audio.paused).toBe(true);
    expect(view.player().snapshot().playing).toBe(false);
    expect(system.session.playbackState).toBe("paused");
    expect(requests.map(({ method, path }) => [method, path])).toEqual([
      ["GET", `/api/v1/recordings/${recording.id}/playback-sources`],
      ["POST", "/api/v1/sync"],
    ]);
    expect(
      requests.every((request) => request.account === "owned-system-account"),
    ).toBe(true);
    const syncs = vi
      .mocked(globalThis.fetch)
      .mock.calls.filter(
        ([input, options]) =>
          String(input) === "/api/v1/sync" && options?.method === "POST",
      );
    expect(syncs).toHaveLength(1);
    const batch = JSON.parse(String(syncs[0][1]?.body)) as Schema<"SyncBatch">;
    expect(batch.operations.map(({ action }) => action)).toEqual([
      {
        kind: "progress_set",
        recording_id: recording.id,
        position_ms: 1000,
        speed: 1,
      },
    ]);
  } finally {
    held?.release();
    view.unmount();
    system.close();
    media.close();
  }
});
