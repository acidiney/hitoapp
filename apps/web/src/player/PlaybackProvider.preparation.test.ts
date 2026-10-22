import { createElement } from "react";
import { act, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { NoticeBanner, NoticeProvider } from "../components/Notices";
import type {
  ResolveSource,
  Source,
  SourceResolution,
  SyncBatch,
} from "../types";
import type { Player, Track } from "./model";
import { PlaybackProvider, usePlayer } from "./PlaybackProvider";
import {
  audioPath,
  browserPreparationMedia,
  deferred,
} from "./fixtures/browserPreparationMedia";

const track = (id: string): Track => ({
  id,
  title: "Owned recording",
  artist: "Owned performer",
  duration_ms: 90000,
  available: true,
});
const first = track("f88f701f-a352-41a0-abda-21fc76b8a387");
const next = track("74aa28ac-1583-4df0-8252-484e6c5ce069");
const after = track("68d155c9-2894-4980-b22b-8855dafb6095");
const source = (recording: Track, id: string): Source => ({
  id,
  recording_id: recording.id,
  provider: "plugin",
  device_id: null,
  ready: true,
  label: "Owned audio service",
  can_prepare: false,
});
const firstSource = source(first, "34aa1ecf-d01b-4b1f-9dab-2f3e40aff28b");
const nextSource = source(next, "563864ed-08b8-41eb-94c3-2948e17d0b0e");
const afterSource = source(after, "2b77f558-ac64-4431-a097-9d141f80f0f9");
const sourcesPath = (recording: Track) =>
  `/api/v1/recordings/${recording.id}/sources`;
const cachedSourcesPath = (recording: Track) =>
  `/api/v1/recordings/${recording.id}/playback-sources`;

type Request = {
  path: string;
  method: string;
  account: string | null;
  body: unknown;
  signal: AbortSignal;
};

function httpBoundary(
  reply: (request: Request) => Response | Promise<Response>,
) {
  const requests: Request[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, options?: RequestInit) => {
      const request: Request = {
        path: String(input),
        method: options?.method ?? "GET",
        account: new Headers(options?.headers).get("x-hito-account"),
        body:
          options?.body == null ? undefined : JSON.parse(String(options.body)),
        signal: options?.signal as AbortSignal,
      };
      requests.push(request);
      if (!(request.signal instanceof AbortSignal))
        throw new Error("Owned playback requests need a cancellation signal");
      return reply(request);
    }),
  );
  return requests;
}

function renderAccount() {
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
        children: createElement(Capture, { account }),
      }),
      createElement(NoticeBanner),
    );
  return { ...render(view("alice")), players, view };
}

async function expectEmptyAccount(player: Player) {
  expect(player.snapshot()).toMatchObject({
    current: null,
    playing: false,
    loading: false,
    error: "",
  });
  expect(player.positionSnapshot()).toEqual({ position: 0, duration: 0 });
  expect(player.queueSnapshot()).toMatchObject({
    total: 0,
    currentEntry: null,
  });
  expect((await player.queuePage(0, 10)).entries).toEqual([]);
  const audio = document.querySelectorAll("audio");
  expect(audio).toHaveLength(1);
  expect(audio[0].hasAttribute("data-hito-player")).toBe(true);
  expect(audio[0].hasAttribute("data-hito-preload")).toBe(false);
  expect(audio[0].getAttribute("src")).toBeNull();
  expect(audio[0].paused).toBe(true);
}

test("an account remount aborts next-source acquisition and ignores its late resolution acknowledgement", async () => {
  const media = browserPreparationMedia();
  const entered = deferred<Request>();
  const acknowledgement = deferred<Response>();
  const requests = httpBoundary((request) => {
    if (request.path === cachedSourcesPath(first) && request.method === "GET")
      return Response.json([firstSource]);
    if (request.path === cachedSourcesPath(next) && request.method === "GET")
      return Response.json([]);
    if (
      request.path === `${sourcesPath(next)}/refresh` &&
      request.method === "POST"
    ) {
      return Response.json([]);
    }
    if (
      request.path === `${sourcesPath(next)}/resolve` &&
      request.method === "POST"
    ) {
      entered.resolve(request);
      return acknowledgement.promise;
    }
    throw new Error(
      "Preparation must not request downloads or other mutations",
    );
  });
  const { players, view, rerender, unmount } = renderAccount();
  try {
    const alice = players.get("alice")!;
    await act(async () => {
      await alice.playQueue([first, next]);
    });
    const pending = await entered.promise;
    expect(pending.signal.aborted).toBe(false);
    expect(media.created).toHaveLength(1);
    expect(media.played.map((play) => play.source)).toEqual([
      audioPath(firstSource.id),
    ]);
    rerender(view("bob"));
    const bob = players.get("bob")!;
    expect(requests.every((request) => request.signal.aborted)).toBe(true);
    expect(media.created[0].isConnected).toBe(false);
    expect(media.created[0].paused).toBe(true);
    expect(media.created[0].getAttribute("src")).toBeNull();
    await expectEmptyAccount(bob);

    await act(async () => {
      const body = pending.body as ResolveSource;
      acknowledgement.resolve(
        Response.json({
          request_id: body.request_id,
          resolution_id: "6065e385-46bb-47cc-ac68-2cadf61b6d7c",
          recording_id: next.id,
          outcome: "ready",
          sources: [nextSource],
        } satisfies SourceResolution),
      );
      await acknowledgement.promise;
    });
    expect(media.created).toHaveLength(2);
    expect(media.played.map((play) => play.source)).toEqual([
      audioPath(firstSource.id),
    ]);
    expect(
      requests.map((request) => `${request.method} ${request.path}`),
    ).toEqual([
      `GET ${cachedSourcesPath(first)}`,
      `GET ${cachedSourcesPath(next)}`,
      `POST ${sourcesPath(next)}/refresh`,
      `POST ${sourcesPath(next)}/resolve`,
    ]);
    expect(requests.every((request) => request.account === "alice")).toBe(true);
    expect(requests[2].body).toEqual({ purpose: "playback" });
    expect(Object.keys(pending.body as ResolveSource)).toEqual(["request_id"]);
    await expectEmptyAccount(bob);
    expect(alice.snapshot().current).toBeNull();
    expect((await alice.queuePage(0, 10)).entries).toEqual([]);
    expect(media.peakOwned()).toBeLessThanOrEqual(2);
  } finally {
    acknowledgement.resolve(Response.json({}));
    unmount();
    media.close();
  }
  expect(
    document.querySelectorAll("audio, [data-hito-player], [data-hito-preload]"),
  ).toHaveLength(0);
});

test("an adopted next buffer is released on account change and late media and progress cannot affect the fresh account", async () => {
  const media = browserPreparationMedia();
  const progress = deferred<Request>();
  const acknowledgement = deferred<Response>();
  const ready = new Map([
    [cachedSourcesPath(first), firstSource],
    [cachedSourcesPath(next), nextSource],
    [cachedSourcesPath(after), afterSource],
  ]);
  const requests = httpBoundary((request) => {
    const audio = ready.get(request.path);
    if (audio && request.method === "GET") return Response.json([audio]);
    if (request.path === "/api/v1/sync" && request.method === "POST") {
      const batch = request.body as SyncBatch;
      const action = batch.operations[0].action;
      if (action.kind === "progress_set" && action.recording_id === next.id) {
        progress.resolve(request);
        return acknowledgement.promise;
      }
      return Response.json({
        acknowledged: batch.operations.map((operation) => operation.id),
      });
    }
    throw new Error(
      "Preparation must not request downloads or other mutations",
    );
  });
  const { players, view, rerender, unmount } = renderAccount();
  try {
    const alice = players.get("alice")!;
    await act(async () => {
      await alice.playQueue([first, next, after]);
    });
    const initial = media.created[0];
    const buffered = await media.loadedSource(nextSource.id);
    const entries = (await alice.queuePage(0, 10)).entries;
    expect(buffered.hasAttribute("data-hito-preload")).toBe(true);
    expect(buffered.paused).toBe(true);
    expect(media.played.map((play) => play.media)).toEqual([initial]);
    expect(document.querySelectorAll("audio")).toHaveLength(2);
    await act(async () => {
      media.canPlay(buffered);
    });
    const adopted = deferred<void>();
    const unsubscribe = alice.subscribe(() => {
      if (alice.snapshot().current?.id === next.id && alice.snapshot().playing)
        adopted.resolve();
    });
    await act(async () => {
      media.end(initial);
      await adopted.promise;
    });
    unsubscribe();
    const following = await media.loadedSource(afterSource.id);
    expect(document.querySelector("audio[data-hito-player]")).toBe(buffered);
    expect(document.querySelector("audio[data-hito-preload]")).toBe(following);
    expect(initial.isConnected).toBe(false);
    expect(initial.paused).toBe(true);
    expect(initial.getAttribute("src")).toBeNull();
    expect(media.played.map((play) => play.media)).toEqual([initial, buffered]);
    expect(
      media.loaded.filter((load) => load.media === buffered && load.source),
    ).toHaveLength(1);
    expect(alice.queueSnapshot().currentEntry).toBe(entries[1].id);
    expect((await alice.queuePage(0, 10)).entries).toEqual(entries);
    act(() => {
      buffered.currentTime = 11;
      buffered.dispatchEvent(new Event("timeupdate"));
    });
    const pendingProgress = await progress.promise;
    expect(pendingProgress.signal.aborted).toBe(false);
    const completed = requests.filter((request) => {
      if (request.path !== "/api/v1/sync" || request.method !== "POST")
        return false;
      const action = (request.body as SyncBatch).operations[0].action;
      return action.kind === "progress_set" && action.recording_id === first.id;
    });
    expect(completed).toHaveLength(1);
    const completedProgress = completed[0];
    expect(completedProgress.body).toEqual({
      device_id: (pendingProgress.body as SyncBatch).device_id,
      operations: [
        {
          id: expect.stringMatching(
            /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i,
          ),
          action: {
            kind: "progress_set",
            recording_id: first.id,
            position_ms: 90000,
            speed: 1,
          },
        },
      ],
    });
    expect((completedProgress.body as SyncBatch).operations[0].id).not.toBe(
      (pendingProgress.body as SyncBatch).operations[0].id,
    );
    expect(requests.indexOf(completedProgress)).toBeLessThan(
      requests.indexOf(pendingProgress),
    );
    rerender(view("bob"));
    const bob = players.get("bob")!;
    const oldMedia = [initial, buffered, following];
    for (const audio of oldMedia) {
      expect(audio.isConnected).toBe(false);
      expect(audio.paused).toBe(true);
      expect(audio.getAttribute("src")).toBeNull();
    }
    expect(pendingProgress.signal.aborted).toBe(true);
    expect(
      requests
        .filter((request) => request !== completedProgress)
        .every((request) => request.signal.aborted),
    ).toBe(true);
    await expectEmptyAccount(bob);
    const publications = vi.fn();
    const stopPlayback = bob.subscribe(publications);
    const stopPosition = bob.subscribePosition(publications);
    const requestCount = requests.length;
    await act(async () => {
      for (const audio of oldMedia) {
        audio.currentTime = 30;
        for (const event of ["play", "timeupdate", "error", "ended", "pause"])
          audio.dispatchEvent(new Event(event));
      }
      acknowledgement.resolve(
        Response.json(
          { error: "Old account progress failed" },
          { status: 503 },
        ),
      );
      await acknowledgement.promise;
    });
    stopPlayback();
    stopPosition();
    expect(publications).not.toHaveBeenCalled();
    expect(requests).toHaveLength(requestCount);
    expect(
      requests.map((request) => `${request.method} ${request.path}`),
    ).toEqual([
      `GET ${cachedSourcesPath(first)}`,
      `GET ${cachedSourcesPath(next)}`,
      "POST /api/v1/sync",
      `GET ${cachedSourcesPath(next)}`,
      `GET ${cachedSourcesPath(after)}`,
      "POST /api/v1/sync",
    ]);
    expect(requests.every((request) => request.account === "alice")).toBe(true);
    expect(
      requests
        .filter((request) => request.path === "/api/v1/sync")
        .flatMap((request) => (request.body as SyncBatch).operations)
        .map((operation) => operation.action),
    ).toEqual([
      {
        kind: "progress_set",
        recording_id: first.id,
        position_ms: 90000,
        speed: 1,
      },
      {
        kind: "progress_set",
        recording_id: next.id,
        position_ms: 11000,
        speed: 1,
      },
    ]);
    expect(media.created).toHaveLength(4);
    expect(media.played.map((play) => play.media)).toEqual([initial, buffered]);
    expect(screen.queryByText("Old account progress failed")).toBeNull();
    await expectEmptyAccount(bob);
    expect(media.peakOwned()).toBe(2);
    unmount();
    expect(
      media.created.every(
        (audio) =>
          !audio.isConnected &&
          audio.paused &&
          audio.getAttribute("src") === null,
      ),
    ).toBe(true);
  } finally {
    acknowledgement.resolve(Response.json({ acknowledged: [] }));
    unmount();
    media.close();
  }
  expect(
    document.querySelectorAll("audio, [data-hito-player], [data-hito-preload]"),
  ).toHaveLength(0);
});
