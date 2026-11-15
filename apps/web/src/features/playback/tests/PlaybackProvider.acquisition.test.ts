import type { Schema } from "../../../shared/schema";
import { createElement } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { NoticeProvider } from "../../../shared/ui/Notices";

import type { Player } from "../model";
import { PlaybackProvider, usePlayer } from "../components/PlaybackProvider";
import PlaybackStatus from "../components/PlaybackStatus";
import { TransportButtons } from "../components/PlaybackControls";
import {
  cachedSourcesPath,
  ownedPlaybackHttp,
  ownedSource,
  ownedTrack,
  sourcesPath,
  unavailableResponse,
  type OwnedPlaybackRequest,
} from "../fixtures/browserSourceResolution";

const first = ownedTrack("f88f701f-a352-41a0-abda-21fc76b8a387");
const firstSource = ownedSource(first, "34aa1ecf-d01b-4b1f-9dab-2f3e40aff28b");

let logged: unknown[][];

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  logged = [];
  for (const level of ["debug", "info", "warn", "error"] as const)
    vi.spyOn(console, level).mockImplementation((...values: unknown[]) => {
      logged.push(values);
    });
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "play").mockImplementation(
    async function (this: HTMLMediaElement) {
      this.dispatchEvent(new Event("play"));
    },
  );
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  expect(JSON.stringify(logged)).not.toContain("Private title canary");
  expect(JSON.stringify(logged)).not.toContain("Private artist canary");
});

function playbackHttp(
  answer: (request: OwnedPlaybackRequest) => Response | Promise<Response>,
) {
  return ownedPlaybackHttp((request) => {
    if (request.path === cachedSourcesPath(first) && request.method === "GET")
      return Response.json([]);
    if (
      request.path === `${sourcesPath(first)}/refresh` &&
      request.method === "POST"
    )
      return Response.json([]);
    if (
      request.path === `${sourcesPath(first)}/resolve` &&
      request.method === "POST"
    )
      return answer(request);
    if (request.path === "/api/v1/sync" && request.method === "POST")
      return Response.json({
        acknowledged: (request.body as Schema<"SyncBatch">).operations.map(
          (operation) => operation.id,
        ),
      });
    if (
      request.path === "/api/v1/source-preferences" &&
      request.method === "GET"
    )
      return Response.json({
        revision: 0,
        fallback: true,
        entries: [],
      } satisfies Schema<"SourcePreferences">);
    throw new Error("Unexpected owned acquisition request");
  });
}

function renderAccount(account: string) {
  const players = new Map<string, Player>();
  function Capture({ account }: { account: string }) {
    players.set(account, usePlayer());
    return null;
  }
  const view = (account: string) =>
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account,
        children: createElement(
          "div",
          null,
          createElement(Capture, { account }),
          createElement(PlaybackStatus),
          createElement(TransportButtons),
        ),
      }),
    );
  return { ...render(view(account)), players, view };
}

test("the Pause control stays enabled while a retry waits and pausing it clears the status and offers Play", async () => {
  const { requests, unexpected } = playbackHttp((request) =>
    unavailableResponse(request, first),
  );
  const { players, unmount } = renderAccount("alice");
  expect(players.get("alice")).toBeDefined();
  const player = players.get("alice")!;
  try {
    await act(async () => {
      void player.playQueue([first]);
      await vi.advanceTimersByTimeAsync(0);
    });
    await vi.waitFor(() =>
      expect(screen.getByRole("status").textContent).toBe(
        "Trying again (2 of 3)…",
      ),
    );
    const pause = screen.getByRole("button", { name: "Pause" });
    expect((pause as HTMLButtonElement).disabled).toBe(false);
    const count = requests.length;
    fireEvent.click(pause);
    await vi.waitFor(() => expect(screen.queryByRole("status")).toBeNull());
    const play = screen.getByRole("button", { name: "Play" });
    expect((play as HTMLButtonElement).disabled).toBe(false);
    expect(player.snapshot().current?.id).toBe(first.id);
    await act(() => vi.advanceTimersByTimeAsync(20000));
    expect(requests).toHaveLength(count);
    expect(requests.every((request) => request.signal.aborted)).toBe(true);
    expect(unexpected).toEqual([]);
  } finally {
    unmount();
  }
});

test("the control shows Pause while granted audio has not been heard and while heard audio recovers", async () => {
  const { unexpected } = playbackHttp((request) =>
    Response.json({
      request_id: (request.body as Schema<"ResolveSource">).request_id,
      resolution_id: "6065e385-46bb-47cc-ac68-2cadf61b6d7c",
      recording_id: first.id,
      outcome: "ready",
      sources: [firstSource],
    } satisfies Schema<"SourceResolution">),
  );
  const { players, unmount } = renderAccount("alice");
  expect(players.get("alice")).toBeDefined();
  const player = players.get("alice")!;
  try {
    await act(async () => {
      void player.playQueue([first]);
      await vi.advanceTimersByTimeAsync(0);
    });
    await vi.waitFor(() => expect(player.snapshot().playing).toBe(true));
    expect(screen.getByRole("button", { name: "Pause" })).toBeTruthy();
    expect(player.snapshot().desiredPlaying).toBe(true);
    const audio = document.querySelector("audio")!;
    Object.defineProperty(audio, "paused", {
      configurable: true,
      get: () => false,
    });
    Object.defineProperty(audio, "currentTime", {
      configurable: true,
      get: () => 2,
      set: () => {},
    });
    act(() => {
      audio.dispatchEvent(new Event("timeupdate"));
    });
    expect(player.snapshot().desiredPlaying).toBeUndefined();
    expect(player.snapshot().playing).toBe(true);
    expect(screen.getByRole("button", { name: "Pause" })).toBeTruthy();
    act(() => {
      audio.dispatchEvent(new Event("error"));
    });
    expect(player.snapshot().loading).toBe(true);
    expect(player.snapshot().desiredPlaying).toBe(true);
    const pause = screen.getByRole("button", { name: "Pause" });
    expect((pause as HTMLButtonElement).disabled).toBe(false);
    expect(unexpected).toEqual([]);
  } finally {
    unmount();
  }
});

test("switching account during a held retry aborts it and its late reply cannot reach the new account", async () => {
  let release!: (response: Response) => void;
  let held: OwnedPlaybackRequest | null = null;
  const { requests, unexpected } = playbackHttp((request) => {
    if (
      held === null &&
      requests.filter((r) => r.path.endsWith("/resolve")).length === 1
    )
      return unavailableResponse(request, first);
    held = request;
    return new Promise<Response>((resolve) => {
      release = resolve;
    });
  });
  const { players, view, rerender, unmount } = renderAccount("alice");
  expect(players.get("alice")).toBeDefined();
  const alice = players.get("alice")!;
  try {
    await act(async () => {
      void alice.playQueue([first]);
      await vi.advanceTimersByTimeAsync(0);
    });
    await vi.waitFor(() =>
      expect(alice.snapshot().retrying).toEqual({ attempt: 2, of: 3 }),
    );
    await act(() => vi.advanceTimersByTimeAsync(500));
    await vi.waitFor(() => expect(held).not.toBeNull());
    rerender(view("bob"));
    expect(held!.signal.aborted).toBe(true);
    const bob = players.get("bob")!;
    const before = bob.snapshot();
    const count = requests.length;
    await act(async () => {
      release(unavailableResponse(held!, first));
      await vi.advanceTimersByTimeAsync(20000);
    });
    expect(bob.snapshot()).toBe(before);
    expect(bob.snapshot().retrying ?? null).toBeNull();
    expect(bob.snapshot().current).toBeNull();
    expect(screen.queryByRole("status")).toBeNull();
    expect(requests).toHaveLength(count);
    expect(requests.every((request) => request.account === "alice")).toBe(true);
    expect(unexpected).toEqual([]);
  } finally {
    unmount();
  }
  expect(document.querySelectorAll("audio")).toHaveLength(0);
});
