import type { Schema } from "../../../shared/schema";
import { createElement } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { NoticeProvider } from "../../../shared/ui/Notices";
import TrackToolbar from "../components/TrackToolbar";

import type { Player, Track } from "../model";
import { PlaybackProvider, usePlayer } from "../components/PlaybackProvider";

test("a lost automatic-resolution acknowledgement retries the same request and preserves duplicate queue occurrences", async () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi
    .spyOn(HTMLMediaElement.prototype, "play")
    .mockImplementation(async function (this: HTMLMediaElement) {
      this.dispatchEvent(new Event("play"));
    });
  const account = "2c231196-2cdc-4120-9d95-2d348bf7c937";
  const recording = "1cbe6b12-a9d8-4da5-9ef0-cf518d27c69c";
  const source = "e72c1fd9-1ed1-4ef5-87f0-c64ebf8b7b82";
  const path = `/api/v1/recordings/${recording}/sources`;
  const cachedPath = `/api/v1/recordings/${recording}/playback-sources`;
  const requests: { path: string; method: string; account: string | null }[] =
    [];
  const attempts: { body: string; signal: AbortSignal }[] = [];
  let accepted: Schema<"SourceResolution"> | null = null;
  let release!: (value: Response) => void;
  const acknowledgement = new Promise<Response>((resolve) => {
    release = resolve;
  });
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, options?: RequestInit) => {
      const url = String(input);
      requests.push({
        path: url,
        method: options?.method ?? "GET",
        account: new Headers(options?.headers).get("x-hito-account"),
      });
      expect(options?.credentials).toBe("same-origin");
      if (url === cachedPath && options?.method === "GET")
        return Response.json([]);
      if (url === `${path}/refresh` && options?.method === "POST") {
        expect(JSON.parse(String(options.body))).toEqual({
          purpose: "playback",
        });
        return Response.json([]);
      }
      if (
        url !== `${path}/resolve` ||
        options?.method !== "POST" ||
        typeof options.body !== "string" ||
        !(options.signal instanceof AbortSignal)
      )
        throw new Error("Unexpected owned playback request");
      attempts.push({ body: options.body, signal: options.signal });
      if (accepted === null) {
        const body: Schema<"ResolveSource"> = JSON.parse(options.body);
        accepted = {
          request_id: body.request_id,
          resolution_id: "fcff73a7-3a51-495e-a666-6de3460edd8c",
          recording_id: recording,
          outcome: "ready",
          sources: [
            {
              id: source,
              recording_id: recording,
              provider: "plugin",
              device_id: null,
              ready: true,
              label: "Owned audio service",
              can_prepare: false,
              match_kind: "verified",
            },
          ],
        };
        throw new TypeError("Owned acknowledgement lost after acceptance");
      }
      return acknowledgement;
    }),
  );
  let player!: Player;
  function Capture() {
    player = usePlayer();
    return null;
  }
  const track: Track = {
    id: recording,
    title: "Personal display title",
    artist: "Personal display artist",
    duration_ms: 12000,
    available: false,
  };
  const { unmount } = render(
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account,
        children: createElement(
          "div",
          null,
          createElement(Capture),
          createElement(TrackToolbar, {
            tracks: [track, track],
            label: "playlist",
            description: "Owned imported playlist",
          }),
        ),
      }),
    ),
  );
  expect(requests).toHaveLength(0);
  const button = screen.getByRole("button", {
    name: "Play playlist",
  }) as HTMLButtonElement;
  expect(button.disabled).toBe(false);
  fireEvent.click(button);
  await vi.waitFor(() => expect(attempts).toHaveLength(2));
  expect(attempts[1].body).toBe(attempts[0].body);
  const body = JSON.parse(attempts[0].body);
  expect(Object.keys(body)).toEqual(["request_id"]);
  expect(body.request_id).toMatch(
    /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i,
  );
  expect(body.request_id).not.toBe("00000000-0000-0000-0000-000000000000");
  expect(attempts.every((attempt) => !attempt.signal.aborted)).toBe(true);
  expect(play).not.toHaveBeenCalled();
  const before = await player.queuePage(0, 10);
  expect(before.entries.map((entry) => entry.recording.id)).toEqual([
    recording,
    recording,
  ]);
  expect(new Set(before.entries.map((entry) => entry.id)).size).toBe(2);
  expect(player.snapshot().loading).toBe(true);
  await act(async () => {
    release(Response.json(accepted));
    await acknowledgement;
  });
  await vi.waitFor(() => expect(player.snapshot().playing).toBe(true));
  expect(requests).toEqual([
    { path: cachedPath, method: "GET", account },
    { path: `${path}/refresh`, method: "POST", account },
    { path: `${path}/resolve`, method: "POST", account },
    { path: `${path}/resolve`, method: "POST", account },
  ]);
  expect(attempts).toHaveLength(2);
  expect(play).toHaveBeenCalledTimes(1);
  const audio = document.querySelector("audio");
  expect(audio).not.toBeNull();
  expect(new URL(audio!.src).origin).toBe(window.location.origin);
  expect(new URL(audio!.src).pathname).toBe(`/api/v1/sources/${source}/audio`);
  expect(player.snapshot().current?.id).toBe(recording);
  expect(player.snapshot().current?.available).toBe(false);
  expect(player.snapshot().error).toBe("");
  const after = await player.queuePage(0, 10);
  expect(after.entries).toEqual(before.entries);
  expect(player.queueSnapshot().currentEntry).toBe(before.entries[0].id);
  unmount();
  expect(attempts.every((attempt) => attempt.signal.aborted)).toBe(true);
  expect(document.querySelectorAll("audio")).toHaveLength(0);
});
