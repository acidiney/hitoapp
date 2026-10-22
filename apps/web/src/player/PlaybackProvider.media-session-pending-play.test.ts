import { act } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { browserPreparationMedia } from "./fixtures/browserPreparationMedia";
import { browserMediaSession } from "./fixtures/browserMediaSession";
import {
  playbackRequests,
  recording,
  renderAccount,
} from "./fixtures/browserMediaSessionPlayback";

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
    expect(requests).toHaveLength(1);
  } finally {
    held?.release();
    view.unmount();
    system.close();
    media.close();
  }
});
