import { act } from "@testing-library/react";
import { expect, test } from "vitest";
import { browserPreparationMedia } from "../fixtures/browserPreparationMedia";
import { browserMediaSession } from "../fixtures/browserMediaSession";
import {
  playbackRequests,
  recording,
  renderAccount,
} from "../fixtures/browserMediaSessionPlayback";

test("browser playback publishes the playing recording and handles system pause, resume, and seeking", async () => {
  const media = browserPreparationMedia();
  const system = browserMediaSession();
  const requests = playbackRequests();
  const view = renderAccount();
  try {
    expect(system.session.metadata).toBeNull();
    expect(system.session.playbackState).toBe("none");
    await act(async () => {
      await view.player().playQueue([recording]);
      media.canPlay(media.created[0]);
    });
    expect(system.session.metadata).toMatchObject({
      title: recording.title,
      artist: recording.artist,
      album: "",
    });
    expect(system.session.playbackState).toBe("playing");
    expect(system.session.position).toEqual({
      duration: 90,
      position: 0,
      playbackRate: 1,
    });

    act(() => {
      media.created[0].currentTime = 4;
      media.created[0].dispatchEvent(new Event("timeupdate"));
    });
    expect(system.session.position).toEqual({
      duration: 90,
      position: 4,
      playbackRate: 1,
    });
    act(() => system.invoke("seekto", { seekTime: 7 }));
    expect(view.player().positionSnapshot().position).toBe(7);
    expect(media.created[0].currentTime).toBe(7);
    expect(system.session.position?.position).toBe(7);

    await act(async () => system.invoke("pause"));
    expect(view.player().snapshot().playing).toBe(false);
    expect(system.session.playbackState).toBe("paused");
    const playCount = media.played.length;
    await act(async () => system.invoke("pause"));
    expect(media.played).toHaveLength(playCount);
    await act(async () => system.invoke("play"));
    expect(view.player().snapshot().playing).toBe(true);
    expect(system.session.playbackState).toBe("playing");
    await act(async () => system.invoke("play"));
    expect(media.played).toHaveLength(playCount + 1);
    act(() => view.player().speed(1.5));
    expect(system.session.position).toEqual({
      duration: 90,
      position: 7,
      playbackRate: 1.5,
    });

    expect(requests.filter((request) => request.method === "GET")).toEqual([
      {
        path: `/api/v1/recordings/${recording.id}/playback-sources`,
        method: "GET",
        account: "owned-system-account",
      },
    ]);
    expect(
      requests.every(
        (request) =>
          request.path === "/api/v1/sync" ||
          request.path.endsWith("/playback-sources"),
      ),
    ).toBe(true);
  } finally {
    view.unmount();
    system.close();
    media.close();
  }
});
