import { createElement } from "react";
import { act, render } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { NoticeProvider } from "../../../shared/ui/Notices";
import type { Player } from "../model";
import { PlaybackProvider, usePlayer } from "../components/PlaybackProvider";

test("browser audio recovery sends the playback refresh purpose and releases account resources", async () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi
    .spyOn(HTMLMediaElement.prototype, "play")
    .mockImplementation(async function (this: HTMLMediaElement) {
      if (this.src.includes("/expired/audio"))
        throw new Error("Owned cached audio is unavailable");
      this.dispatchEvent(new Event("play"));
    });
  const source = (id: string) => ({
    id,
    recording_id: "saved",
    provider: "plugin",
    device_id: null,
    ready: true,
  });
  const fetch = vi.fn(
    async (input: RequestInfo | URL, _options?: RequestInit) => {
      if (input === "/api/v1/recordings/saved/playback-sources")
        return new Response(JSON.stringify([source("expired")]));
      if (input === "/api/v1/source-preferences")
        return new Response(
          JSON.stringify({ revision: 0, fallback: true, entries: [] }),
        );
      if (input === "/api/v1/recordings/saved/sources/refresh")
        return new Response(JSON.stringify([source("replacement")]));
      throw new Error("Unexpected owned playback request");
    },
  );
  vi.stubGlobal("fetch", fetch);
  let player!: Player;
  function Capture() {
    player = usePlayer();
    return null;
  }
  const { unmount } = render(
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account: "alice",
        children: createElement(Capture),
      }),
    ),
  );
  await act(async () => {
    await player.playQueue([
      {
        id: "saved",
        title: "Owned track",
        artist: "Owned artist",
        duration_ms: 90000,
        available: true,
      },
    ]);
  });
  expect(play).toHaveBeenCalledTimes(2);
  expect(document.querySelector("audio")?.src).toContain("/replacement/audio");
  expect(player.snapshot().current?.id).toBe("saved");
  expect(player.snapshot().playing).toBe(true);
  expect(fetch).toHaveBeenCalledTimes(3);
  expect(fetch.mock.calls.map(([input]) => input)).toEqual([
    "/api/v1/recordings/saved/playback-sources",
    "/api/v1/source-preferences",
    "/api/v1/recordings/saved/sources/refresh",
  ]);
  const cached = fetch.mock.calls[0][1];
  const recovery = fetch.mock.calls[2][1];
  expect(cached).toEqual(
    expect.objectContaining({ method: "GET", body: undefined }),
  );
  expect(recovery).toEqual(
    expect.objectContaining({
      method: "POST",
      body: JSON.stringify({ purpose: "playback" }),
      credentials: "same-origin",
      headers: expect.objectContaining({ "x-hito-account": "alice" }),
      signal: expect.any(AbortSignal),
    }),
  );
  expect(recovery?.signal?.aborted).toBe(false);
  unmount();
  expect(cached?.signal?.aborted).toBe(true);
  expect(recovery?.signal?.aborted).toBe(true);
  expect(document.querySelectorAll("audio")).toHaveLength(0);
});
