import type { Schema } from "../../../shared/schema";
import { createElement } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { NoticeProvider } from "../../../shared/ui/Notices";
import TrackToolbar from "../components/TrackToolbar";

import type { Player, Track } from "../model";
import { PlaybackProvider, usePlayer } from "../components/PlaybackProvider";
import PlaybackStatus from "../components/PlaybackStatus";

test("the outage notice clears once the same track plays after going back to it", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  const logged: unknown[][] = [];
  vi.spyOn(console, "info").mockImplementation((...values) =>
    logged.push(values),
  );
  vi.spyOn(console, "warn").mockImplementation((...values) =>
    logged.push(values),
  );
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi
    .spyOn(HTMLMediaElement.prototype, "play")
    .mockImplementation(async function (this: HTMLMediaElement) {
      this.dispatchEvent(new Event("play"));
    });
  const account = "2c231196-2cdc-4120-9d95-2d348bf7c937";
  const recording = "1cbe6b12-a9d8-4da5-9ef0-cf518d27c69c";
  const source: Schema<"Source"> = {
    id: "e72c1fd9-1ed1-4ef5-87f0-c64ebf8b7b82",
    recording_id: recording,
    provider: "Private provider canary",
    device_id: null,
    ready: true,
    label: "Private source label canary",
    can_prepare: false,
    match_kind: "verified",
  };
  const cachedPath = `/api/v1/recordings/${recording}/playback-sources`;
  const sourcePath = `/api/v1/recordings/${recording}/sources`;
  const requests: { path: string; method: string; account: string | null }[] =
    [];
  const attempts: { body: Schema<"ResolveSource">; signal: AbortSignal }[] = [];
  const unexpected: string[] = [];
  let available = false;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, options?: RequestInit) => {
      const path = String(input);
      requests.push({
        path,
        method: options?.method ?? "GET",
        account: new Headers(options?.headers).get("x-hito-account"),
      });
      expect(options?.credentials).toBe("same-origin");
      if (path === cachedPath && options?.method === "GET")
        return Response.json([]);
      if (path === `${sourcePath}/refresh` && options?.method === "POST") {
        expect(JSON.parse(String(options.body))).toEqual({
          purpose: "playback",
        });
        return Response.json([]);
      }
      if (
        path !== `${sourcePath}/resolve` ||
        options?.method !== "POST" ||
        typeof options.body !== "string" ||
        !(options.signal instanceof AbortSignal)
      ) {
        unexpected.push(path);
        throw new Error("Unexpected owned playback request");
      }
      const body: Schema<"ResolveSource"> = JSON.parse(options.body);
      expect(Object.keys(body)).toEqual(["request_id"]);
      expect(body.request_id).toMatch(
        /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i,
      );
      attempts.push({ body, signal: options.signal });
      return Response.json({
        request_id: body.request_id,
        resolution_id: "fcff73a7-3a51-495e-a666-6de3460edd8c",
        recording_id: recording,
        outcome: available ? "ready" : "provider_unavailable",
        sources: available ? [source] : [],
      });
    }),
  );
  let player!: Player;
  function Capture() {
    player = usePlayer();
    return null;
  }
  const track: Track = {
    id: recording,
    title: "Private display title",
    artist: "Private display artist",
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
          createElement(PlaybackStatus),
          createElement(TrackToolbar, {
            tracks: [track],
            label: "playlist",
            description: "Owned imported playlist",
          }),
        ),
      }),
    ),
  );
  expect(requests).toHaveLength(0);
  const pass = [
    { path: cachedPath, method: "GET", account },
    { path: `${sourcePath}/refresh`, method: "POST", account },
    { path: `${sourcePath}/resolve`, method: "POST", account },
  ];
  fireEvent.click(screen.getByRole("button", { name: "Play playlist" }));
  await vi.waitFor(() =>
    expect(player.snapshot().retrying).toEqual({ attempt: 2, of: 3 }),
  );
  expect(requests).toEqual(pass);
  await act(() => vi.advanceTimersByTimeAsync(500));
  await vi.waitFor(() =>
    expect(player.snapshot().retrying).toEqual({ attempt: 3, of: 3 }),
  );
  expect(requests).toEqual([...pass, ...pass]);
  await act(() => vi.advanceTimersByTimeAsync(1000));
  await vi.waitFor(() => {
    expect(player.snapshot().loading).toBe(false);
    expect(player.snapshot().error).not.toBe("");
  });
  expect(attempts).toHaveLength(3);
  expect(unexpected).toEqual([]);
  expect(requests).toEqual([...pass, ...pass, ...pass]);
  expect(screen.getByRole("status").textContent).toBe(
    "An audio service is unavailable. Select the track again to retry.",
  );
  expect(play).not.toHaveBeenCalled();
  const queue = await player.queuePage(0, 10);
  expect(queue.entries.map((entry) => entry.recording)).toEqual([track]);
  expect(player.snapshot().playing).toBe(false);

  available = true;
  await player.previous();
  await vi.waitFor(() => expect(player.snapshot().playing).toBe(true));
  expect(player.snapshot().current?.id).toBe(recording);
  expect(player.snapshot().error).toBe("");
  expect(screen.queryByRole("status")?.textContent ?? "").toBe("");
  expect(unexpected).toEqual([]);
  unmount();
  vi.useRealTimers();
});
