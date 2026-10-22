import { afterEach, expect, test, vi } from "vitest";
import { Playback, type MediaPort, type Track } from "./playback";
import type { BrowserMediaOwner } from "./player/BrowserPreparation";
import { log } from "./shared/log";

afterEach(() => vi.restoreAllMocks());

class Media extends EventTarget implements MediaPort {
  src = "";
  currentSrc = "";
  currentTime = 0;
  duration = 440;
  playbackRate = 1;
  defaultPlaybackRate = 1;
  volume = 1;
  paused = true;
  readyState = 0;
  seeking = false;
  async play() {
    this.paused = false;
    this.dispatchEvent(new Event("play"));
  }
  pause() {
    this.paused = true;
    this.dispatchEvent(new Event("pause"));
  }
  load() {
    this.currentSrc = "";
    this.currentTime = 0;
  }
  removeAttribute() {
    this.src = "";
  }
  metadata() {
    this.currentSrc = this.src;
    this.readyState = 2;
    this.dispatchEvent(new Event("loadedmetadata"));
  }
}

const track = (id: string): Track => ({
  id,
  title: "Owned duration fixture",
  artist: "Owned performer",
  duration_ms: 440_000,
  available: true,
});

test("a current source revises duration without changing heard progress or playback identity", async () => {
  const media = new Media();
  const progress = vi.fn();
  const resolve = vi.fn(async (id: string) => [
    { id: `source-${id}`, ready: true },
  ]);
  const player = new Playback(media, resolve, progress);
  const tracks = [track("first"), track("next")];
  await player.playQueue(tracks);
  media.metadata();
  player.speed(1.25);
  media.currentTime = 11.25;
  media.dispatchEvent(new Event("timeupdate"));
  expect(player.positionSnapshot()).toEqual({ position: 11.25, duration: 440 });
  expect(progress.mock.calls).toEqual([[tracks[0], 11.25, 1.25]]);
  const state = player.snapshot();
  const queue = await player.queuePage(0, 50);
  const queueView = player.queueSnapshot();
  const source = media.src;
  const positions = vi.fn();
  const structure = vi.fn();
  player.subscribePosition(positions);
  player.subscribe(structure);
  const logs = vi.spyOn(log, "debug");

  media.duration = 220;
  media.dispatchEvent(new Event("durationchange"));

  expect(player.positionSnapshot()).toEqual({ position: 11.25, duration: 220 });
  expect(player.snapshot()).toEqual({ ...state, duration: 220 });
  expect(await player.queuePage(0, 50)).toEqual(queue);
  expect(player.queueSnapshot()).toBe(queueView);
  expect(media.src).toBe(source);
  expect(media.currentTime).toBe(11.25);
  expect(media.playbackRate).toBe(1.25);
  expect(media.paused).toBe(false);
  expect(resolve).toHaveBeenCalledTimes(1);
  expect(progress.mock.calls).toEqual([[tracks[0], 11.25, 1.25]]);
  expect(positions).toHaveBeenCalledTimes(1);
  expect(structure).not.toHaveBeenCalled();
  expect(logs).toHaveBeenCalledExactlyOnceWith("playback.duration_changed", {
    recording_id: tracks[0].id,
    reason: "finite",
    duration_ms: expect.any(Number),
  });
  expect(logs.mock.calls[0][1]!.duration_ms).toBeGreaterThanOrEqual(0);
});

test.each([Number.NaN, Number.POSITIVE_INFINITY, -1])(
  "a duration revision to %s reports unknown without seeking or saving progress",
  async (unknown) => {
    const media = new Media();
    const progress = vi.fn();
    const player = new Playback(
      media,
      async () => [{ id: "owned-source", ready: true }],
      progress,
    );
    await player.playQueue([track("owned")]);
    media.metadata();
    player.seek(15);
    media.seeking = true;
    const positions = vi.fn();
    const structure = vi.fn();
    player.subscribePosition(positions);
    player.subscribe(structure);

    media.duration = unknown;
    media.dispatchEvent(new Event("durationchange"));
    expect(player.positionSnapshot()).toEqual({ position: 15, duration: 0 });
    expect(media.currentTime).toBe(15);
    expect(media.seeking).toBe(true);
    expect(progress).not.toHaveBeenCalled();
    expect(positions).toHaveBeenCalledTimes(1);

    media.dispatchEvent(new Event("durationchange"));
    expect(positions).toHaveBeenCalledTimes(1);
    media.duration = 220;
    media.dispatchEvent(new Event("durationchange"));
    expect(player.positionSnapshot()).toEqual({ position: 15, duration: 220 });
    expect(media.currentTime).toBe(15);
    expect(media.seeking).toBe(true);
    expect(positions).toHaveBeenCalledTimes(2);
    expect(structure).not.toHaveBeenCalled();
    expect(progress).not.toHaveBeenCalled();
  },
);

test("retired source and account duration events cannot overwrite the current owner", async () => {
  const media = new Media();
  let finish!: (sources: { id: string; ready: boolean }[]) => void;
  let entered!: () => void;
  const waiting = new Promise<void>((resolve) => {
    entered = resolve;
  });
  const player = new Playback(media, async (id) => {
    if (id === "next") {
      entered();
      return new Promise((resolve) => {
        finish = resolve;
      });
    }
    return [{ id: "first-source", ready: true }];
  });
  await player.playQueue([track("first"), track("next")]);
  media.metadata();
  const oldSource = media.currentSrc;
  const opening = player.next();
  await waiting;
  const loading = player.positionSnapshot();
  media.currentSrc = oldSource;
  media.duration = 99;
  media.dispatchEvent(new Event("durationchange"));
  expect(player.positionSnapshot()).toBe(loading);

  finish([{ id: "next-source", ready: true }]);
  await opening;
  const next = player.positionSnapshot();
  media.currentSrc = oldSource;
  media.dispatchEvent(new Event("durationchange"));
  expect(player.positionSnapshot()).toBe(next);
  media.duration = 220;
  media.metadata();
  media.duration = 180;
  media.dispatchEvent(new Event("durationchange"));
  expect(player.positionSnapshot()).toEqual({ position: 0, duration: 180 });

  player.retire();
  const retired = player.positionSnapshot();
  const positions = vi.fn();
  player.subscribePosition(positions);
  const accountMedia = new Media();
  const replacement = new Playback(accountMedia, async () => [
    { id: "other-account-source", ready: true },
  ]);
  await replacement.playQueue([track("other-account")]);
  accountMedia.duration = 110;
  accountMedia.metadata();
  accountMedia.duration = 100;
  accountMedia.dispatchEvent(new Event("durationchange"));
  media.duration = 99;
  media.dispatchEvent(new Event("durationchange"));
  expect(replacement.positionSnapshot()).toEqual({
    position: 0,
    duration: 100,
  });
  expect(player.positionSnapshot()).toEqual(retired);
  expect(player.snapshot().current).toBeNull();
  expect(positions).not.toHaveBeenCalled();
});

test("prepared media owns duration revisions after handoff and retired media stays silent", async () => {
  const initial = new Media();
  const prepared = new Media();
  prepared.duration = 220;
  let created!: () => void;
  const ready = new Promise<void>((resolve) => {
    created = resolve;
  });
  const owner: BrowserMediaOwner = {
    create() {
      created();
      return prepared;
    },
    activate: vi.fn(),
    release: vi.fn(),
  };
  const player = new Playback(
    initial,
    async (id) => [{ id: `source-${id}`, ready: true }],
    undefined,
    owner,
  );
  await player.playQueue([track("first"), track("next")]);
  initial.metadata();
  await ready;
  prepared.metadata();
  prepared.dispatchEvent(new Event("canplay"));
  await player.next();
  expect(owner.activate).toHaveBeenCalledWith(prepared);
  expect(owner.release).toHaveBeenCalledWith(initial);
  const current = player.positionSnapshot();
  const positions = vi.fn();
  player.subscribePosition(positions);

  initial.duration = 999;
  initial.dispatchEvent(new Event("durationchange"));
  expect(player.positionSnapshot()).toBe(current);
  expect(positions).not.toHaveBeenCalled();
  prepared.duration = 180;
  prepared.dispatchEvent(new Event("durationchange"));
  expect(player.positionSnapshot()).toEqual({ position: 0, duration: 180 });
  expect(positions).toHaveBeenCalledTimes(1);
  expect(player.snapshot().current?.id).toBe("next");
  player.retire();
});
