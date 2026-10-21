// @vitest-environment jsdom
import { createElement } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { NoticeProvider } from "../components/Notices";
import TrackToolbar from "../components/TrackToolbar";
import type { ResolveSource, Source } from "../types";
import type { Player, Track } from "./model";
import { PlaybackProvider, usePlayer } from "./PlaybackProvider";
import PlaybackStatus from "./PlaybackStatus";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("an accepted service outage is shown once and selecting Play again makes a new resolution request", async () => {
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
  const source: Source = {
    id: "e72c1fd9-1ed1-4ef5-87f0-c64ebf8b7b82",
    recording_id: recording,
    provider: "Private provider canary",
    device_id: null,
    ready: true,
    label: "Private source label canary",
    can_prepare: false,
  };
  const cachedPath = `/api/v1/recordings/${recording}/playback-sources`;
  const sourcePath = `/api/v1/recordings/${recording}/sources`;
  const requests: { path: string; method: string; account: string | null }[] =
    [];
  const attempts: { body: ResolveSource; signal: AbortSignal }[] = [];
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
      const body: ResolveSource = JSON.parse(options.body);
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
  fireEvent.click(screen.getByRole("button", { name: "Play playlist" }));
  await vi.waitFor(() => {
    expect(player.snapshot().loading).toBe(false);
    expect(player.snapshot().error).not.toBe("");
  });
  expect(attempts).toHaveLength(1);
  expect(unexpected).toEqual([]);
  expect(requests).toEqual([
    { path: cachedPath, method: "GET", account },
    { path: `${sourcePath}/refresh`, method: "POST", account },
    { path: `${sourcePath}/resolve`, method: "POST", account },
  ]);
  expect(screen.getByRole("status").textContent).toBe(
    "An audio service is unavailable. Select the track again to retry.",
  );
  expect(play).not.toHaveBeenCalled();
  const queue = await player.queuePage(0, 10);
  expect(queue.entries.map((entry) => entry.recording)).toEqual([track]);
  expect(player.snapshot().playing).toBe(false);

  available = true;
  fireEvent.click(screen.getByRole("button", { name: "Play playlist" }));
  await vi.waitFor(() => expect(player.snapshot().playing).toBe(true));
  expect(attempts).toHaveLength(2);
  expect(attempts[1].body.request_id).not.toBe(attempts[0].body.request_id);
  expect(attempts[0].signal.aborted).toBe(true);
  expect(attempts[1].signal.aborted).toBe(false);
  expect(player.snapshot().error).toBe("");
  expect(player.snapshot().current?.id).toBe(recording);
  expect(play).toHaveBeenCalledTimes(1);
  expect(new URL(document.querySelector("audio")!.src).pathname).toBe(
    `/api/v1/sources/${source.id}/audio`,
  );
  expect(unexpected).toEqual([]);
  expect(requests).toHaveLength(6);
  const completions = logged
    .map((values) => values[1])
    .filter(
      (value): value is Record<string, unknown> =>
        value !== null &&
        typeof value === "object" &&
        "event" in value &&
        value.event === "playback.source_resolution_completed",
    );
  expect(completions.map((event) => event.reason)).toEqual([
    "provider_unavailable",
    "ready",
  ]);
  expect(completions[0]).toEqual(
    expect.objectContaining({
      account_id: account,
      request_id: attempts[0].body.request_id,
      operation_id: "fcff73a7-3a51-495e-a666-6de3460edd8c",
      duration_ms: expect.any(Number),
      count: 0,
    }),
  );
  for (const secret of [
    track.title,
    track.artist,
    source.provider,
    source.label,
  ])
    expect(JSON.stringify(logged)).not.toContain(secret);
  unmount();
  expect(attempts.every((attempt) => attempt.signal.aborted)).toBe(true);
  expect(document.querySelectorAll("audio")).toHaveLength(0);
});
