import { act } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { browserPreparationMedia } from "../fixtures/browserPreparationMedia";
import { browserMediaSession } from "../fixtures/browserMediaSession";
import {
  playbackRequests,
  recording,
  renderAccount,
} from "../fixtures/browserMediaSessionPlayback";

function ownedAudio() {
  const audio = document.querySelector<HTMLAudioElement>(
    "audio[data-hito-player]",
  );
  if (!audio) throw new Error("An owned browser player must be mounted");
  return audio;
}

test("clearing or naturally completing the queue removes private system metadata and transport handlers", async () => {
  const media = browserPreparationMedia();
  const system = browserMediaSession();
  const requests = playbackRequests();
  const view = renderAccount();
  try {
    await act(async () => {
      await view.player().playQueue([recording]);
    });
    expect(system.session.metadata?.title).toBe(recording.title);
    const oldHandlers = [...system.handlers];
    expect(oldHandlers.length).toBeGreaterThan(0);
    act(() => view.player().clear());
    expect(system.session.metadata).toBeNull();
    expect(system.session.playbackState).toBe("none");
    expect(system.session.position).toBeUndefined();
    expect(system.handlers.size).toBe(0);
    const requestCount = requests.length;
    await act(async () => {
      for (const [action, callback] of oldHandlers)
        callback({ action, seekTime: 42 });
    });
    expect(system.session.metadata).toBeNull();
    expect(requests).toHaveLength(requestCount);

    await act(async () => {
      await view.player().playQueue([recording]);
      media.canPlay(ownedAudio());
    });
    expect(system.session.playbackState).toBe("playing");
    await act(async () => media.end(ownedAudio()));
    expect(view.player().snapshot().current).toBeNull();
    expect(system.session.metadata).toBeNull();
    expect(system.session.playbackState).toBe("none");
    expect(system.session.position).toBeUndefined();
    expect(system.handlers.size).toBe(0);
  } finally {
    view.unmount();
    system.close();
    media.close();
  }
});

test("account replacement clears system state and old transport callbacks and media events cannot affect the new account", async () => {
  const media = browserPreparationMedia();
  const system = browserMediaSession();
  const requests = playbackRequests();
  const info = vi.spyOn(console, "info").mockImplementation(() => {});
  const warnings = vi.spyOn(console, "warn").mockImplementation(() => {});
  const view = renderAccount();
  try {
    await act(async () => {
      await view.player().playQueue([recording]);
      media.canPlay(ownedAudio());
    });
    const oldAudio = ownedAudio();
    const oldHandlers = [...system.handlers];
    view.rerender(view.accountView("second-owned-account"));
    expect(system.session.metadata).toBeNull();
    expect(system.session.playbackState).toBe("none");
    expect(system.session.position).toBeUndefined();
    expect(system.handlers.size).toBe(0);
    const replacement = { ...recording, title: "Second account private title" };
    await act(async () => {
      await view.player().playQueue([replacement]);
      media.canPlay(ownedAudio());
    });
    const metadata = system.session.metadata;
    const positions = system.session.setPositionState.mock.calls.length;
    const requestCount = requests.length;
    const plays = media.played.length;
    await act(async () => {
      for (const [action, callback] of oldHandlers)
        callback({ action, seekTime: 42 });
      oldAudio.currentTime = 42;
      for (const event of ["play", "timeupdate", "pause", "ended"])
        oldAudio.dispatchEvent(new Event(event));
    });
    expect(system.session.metadata).toBe(metadata);
    expect(system.session.metadata?.title).toBe(replacement.title);
    expect(system.session.playbackState).toBe("playing");
    expect(system.session.setPositionState).toHaveBeenCalledTimes(positions);
    expect(view.player().positionSnapshot().position).toBe(0);
    expect(requests).toHaveLength(requestCount);
    expect(media.played).toHaveLength(plays);
    expect(requests.map((request) => request.account)).toEqual([
      "owned-system-account",
      "second-owned-account",
    ]);

    const latestHandlers = [...system.handlers];
    view.unmount();
    expect(system.session.metadata).toBeNull();
    expect(system.session.playbackState).toBe("none");
    expect(system.session.position).toBeUndefined();
    expect(system.handlers.size).toBe(0);
    await act(async () => {
      for (const [action, callback] of latestHandlers)
        callback({ action, seekTime: 17 });
    });
    expect(requests).toHaveLength(requestCount);
    expect(media.played).toHaveLength(plays);
    const logs = JSON.stringify([info.mock.calls, warnings.mock.calls]);
    for (const privateText of [
      recording.title,
      recording.artist,
      replacement.title,
    ])
      expect(logs).not.toContain(privateText);
  } finally {
    view.unmount();
    system.close();
    media.close();
  }
});

test("Strict Mode replay keeps one browser owner and position updates do not rerender provider children", async () => {
  const media = browserPreparationMedia();
  const system = browserMediaSession();
  const requests = playbackRequests();
  const view = renderAccount(true);
  try {
    expect(document.querySelectorAll("audio[data-hito-player]")).toHaveLength(
      1,
    );
    expect(requests).toHaveLength(0);
    await act(async () => {
      await view.player().playQueue([recording]);
      media.canPlay(ownedAudio());
    });
    const renders = view.renders();
    act(() => {
      for (let position = 1; position <= 9; position++) {
        ownedAudio().currentTime = position;
        ownedAudio().dispatchEvent(new Event("timeupdate"));
      }
    });
    expect(view.renders()).toBe(renders);
    expect(system.session.position?.position).toBe(9);
    expect(system.session.metadata?.title).toBe(recording.title);
    await act(async () => system.invoke("pause"));
    expect(view.player().snapshot().playing).toBe(false);
    expect(system.session.playbackState).toBe("paused");
    view.unmount();
    expect(document.querySelectorAll("audio")).toHaveLength(0);
    expect(system.session.metadata).toBeNull();
    expect(system.handlers.size).toBe(0);
  } finally {
    view.unmount();
    system.close();
    media.close();
  }
});

test("partial system support rejects only unsupported controls and does not break ordinary playback", async () => {
  const media = browserPreparationMedia();
  const system = browserMediaSession();
  system.session.setActionHandler.mockImplementation((action, callback) => {
    if (action === "seekto") throw new Error("Private platform error body");
    if (callback) system.handlers.set(action, callback);
    else system.handlers.delete(action);
  });
  Reflect.deleteProperty(system.session, "setPositionState");
  vi.stubGlobal("MediaMetadata", undefined);
  const requests = playbackRequests();
  const warnings = vi.spyOn(console, "warn").mockImplementation(() => {});
  const view = renderAccount();
  try {
    await act(async () => {
      await view.player().playQueue([recording]);
    });
    expect(view.player().snapshot().playing).toBe(true);
    expect(system.session.playbackState).toBe("playing");
    expect(system.handlers.has("seekto")).toBe(false);
    await act(async () => system.invoke("pause"));
    expect(view.player().snapshot().playing).toBe(false);
    expect(system.session.playbackState).toBe("paused");
    expect(requests).toHaveLength(1);
    expect(JSON.stringify(warnings.mock.calls)).not.toContain(
      "Private platform error body",
    );
  } finally {
    view.unmount();
    system.close();
    media.close();
  }
});

test("an unsupported browser still plays and invalid duration never publishes or accepts a system seek", async () => {
  const media = browserPreparationMedia();
  const system = browserMediaSession();
  const requests = playbackRequests();
  Object.defineProperty(navigator, "mediaSession", {
    configurable: true,
    value: undefined,
  });
  const unsupported = renderAccount();
  try {
    await act(async () => {
      await unsupported.player().playQueue([recording]);
    });
    expect(unsupported.player().snapshot().playing).toBe(true);
    expect(system.session.metadata).toBeNull();
    expect(system.handlers.size).toBe(0);
    unsupported.unmount();
    Object.defineProperty(navigator, "mediaSession", {
      configurable: true,
      value: system.session,
    });
    vi.spyOn(HTMLMediaElement.prototype, "duration", "get").mockReturnValue(
      Number.NaN,
    );
    const view = renderAccount();
    try {
      await act(async () => {
        await view
          .player()
          .playQueue([{ ...recording, duration_ms: Number.NaN }]);
        media.canPlay(ownedAudio());
      });
      expect(system.session.metadata?.title).toBe(recording.title);
      expect(system.session.position).toBeUndefined();
      const count = requests.length;
      act(() => {
        for (const seekTime of [17, Number.NaN, Number.POSITIVE_INFINITY])
          system.invoke("seekto", { seekTime });
      });
      expect(view.player().positionSnapshot().position).toBe(0);
      expect(ownedAudio().currentTime).toBe(0);
      expect(requests).toHaveLength(count);
      expect(
        system.session.setPositionState.mock.calls.every(
          ([position]) => position === undefined,
        ),
      ).toBe(true);
    } finally {
      view.unmount();
    }
  } finally {
    unsupported.unmount();
    system.close();
    media.close();
  }
});
