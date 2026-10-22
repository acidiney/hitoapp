import { createElement } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { NoticeProvider } from "../../../shared/ui/Notices";
import TrackToolbar from "../components/TrackToolbar";
import { audiobookContinuationMedia } from "../../audiobooks/fixtures/audiobookContinuationMedia";
import type { components } from "../../../generated";
import { PlaybackProvider, usePlayer } from "../components/PlaybackProvider";
import type { Player } from "../model";

type Schema = components["schemas"];
test("ordinary browser Play consumes the authenticated playback-source order while the legacy list remains deterministic", async () => {
  const media = audiobookContinuationMedia();
  const recording = "18d29dd6-fc1f-4452-b41a-95c69526e9fb";
  const baseline: Schema["Source"][] = [
    {
      id: "fcd91730-056b-4452-a120-70f7086f8d15",
      recording_id: recording,
      provider: "plugin",
      device_id: null,
      ready: true,
      label: "Owned first source",
      can_prepare: false,
    },
    {
      id: "70dc49bc-eccd-47fe-b38c-8e79cf48df6f",
      recording_id: recording,
      provider: "plugin",
      device_id: null,
      ready: true,
      label: "Owned preferred source",
      can_prepare: false,
    },
  ];
  const ranked = [...baseline].reverse();
  const requests: { path: string; signal: AbortSignal }[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn<typeof fetch>(async (input, options) => {
      expect(options?.method).toBe("GET");
      expect(options?.credentials).toBe("same-origin");
      expect(new Headers(options?.headers).get("x-hito-account")).toBe("alice");
      expect(new Headers(options?.headers).get("x-hito-request")).toBe("1");
      expect(options?.signal).toBeInstanceOf(AbortSignal);
      expect(options?.body).toBeUndefined();
      const path = String(input);
      requests.push({ path, signal: options!.signal as AbortSignal });
      if (path === `/api/v1/recordings/${recording}/sources`)
        return Response.json(baseline);
      if (path === `/api/v1/recordings/${recording}/playback-sources`)
        return Response.json(ranked);
      throw new Error(
        "Cached ready playback must not refresh, resolve, prepare, download or write preferences",
      );
    }),
  );
  let player!: Player;
  function Capture() {
    player = usePlayer();
    return null;
  }
  const track = {
    id: recording,
    title: "Owned fixture",
    artist: "Owned performer",
    duration_ms: 12_000,
    available: true,
  };
  const view = render(
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account: "alice",
        children: createElement(
          "div",
          null,
          createElement(Capture),
          createElement(TrackToolbar, {
            tracks: [track],
            label: "playlist",
            description: "Owned single-entry playlist",
          }),
        ),
      }),
    ),
  );
  try {
    expect(requests).toHaveLength(0);
    fireEvent.click(screen.getByRole("button", { name: "Play playlist" }));
    await vi.waitFor(() =>
      expect(player.snapshot().current?.id).toBe(recording),
    );
    await vi.waitFor(() =>
      expect(new URL(media.current().src).pathname).toBe(
        `/api/v1/sources/${ranked[0].id}/audio`,
      ),
    );
    act(() => {
      media.ready(media.current());
      media.observePlaying(media.current());
    });
    expect(player.snapshot()).toMatchObject({
      playing: true,
      error: "",
      current: { id: recording },
    });
    expect(requests.map((request) => request.path)).toEqual([
      `/api/v1/recordings/${recording}/playback-sources`,
    ]);
    const queue = await player.queuePage(0, 10);
    expect(queue.entries.map((entry) => entry.recording.id)).toEqual([
      recording,
    ]);
    expect(document.querySelectorAll("audio")).toHaveLength(1);
  } finally {
    view.unmount();
    expect(requests.every((request) => request.signal.aborted)).toBe(true);
  }
});
