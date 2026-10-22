import { setImmediate as flushTasks } from "node:timers/promises";
import { expect, test, vi } from "vitest";
import { Playback, type MediaPort, type Track } from "./playback";
import type { Source, SourceResolver } from "./playbackSources";
import type { BrowserMediaOwner } from "./player/BrowserPreparation";
import { deferred } from "./test/async";

class Media extends EventTarget implements MediaPort {
  src = "";
  private position = 0;
  get currentTime() {
    return this.position;
  }
  set currentTime(value: number) {
    this.position = value;
  }
  duration = 90;
  playbackRate = 1;
  defaultPlaybackRate = 1;
  volume = 1;
  paused = true;
  ended = false;
  pauses = 0;
  loads: string[] = [];
  played: string[] = [];

  async play() {
    this.played.push(this.src);
    this.paused = false;
    this.dispatchEvent(new Event("play"));
  }

  pause() {
    this.pauses++;
    this.paused = true;
    this.dispatchEvent(new Event("pause"));
  }

  load() {
    this.loads.push(this.src);
    this.currentTime = 0;
    this.ended = false;
    this.dispatchEvent(new Event("loadstart"));
  }

  removeAttribute(name: string) {
    if (name === "src") this.src = "";
  }
}

const track = (id: string): Track => ({
  id,
  title: "Owned recording",
  artist: "Owned performer",
  duration_ms: 90000,
  available: true,
});
const path = (source: string) => `/api/v1/sources/${source}/audio`;

function mediaOwner(
  initial: Media,
  loaded: (media: Media) => void = () => {},
  create: () => Media = () => new Media(),
) {
  const ports = [initial];
  const owned = new Set<MediaPort>(ports);
  const unexpectedNetwork = vi.fn(async () => {
    throw new Error("Playback must not create a download or other HTTP work");
  });
  vi.stubGlobal("fetch", unexpectedNetwork);
  const owner: BrowserMediaOwner = {
    create: vi.fn(() => {
      const media = create();
      ports.push(media);
      owned.add(media);
      media.addEventListener("loadstart", () => loaded(media));
      return media;
    }),
    activate: (media) => {
      expect(owned.has(media)).toBe(true);
    },
    release: (media) => {
      expect(media.paused).toBe(true);
      expect(media.src).toBe("");
      expect(owned.delete(media)).toBe(true);
    },
  };
  return { ports, owned, owner, unexpectedNetwork };
}

test("a changed authorized source retires a buffered choice before queue handoff without matching again", async () => {
  const initial = new Media();
  const buffered = deferred<Media>();
  const { ports, owned, owner, unexpectedNetwork } = mediaOwner(
    initial,
    (media) => {
      if (media.src === path("previous-choice")) buffered.resolve(media);
    },
  );
  const nextSources = deferred<Source[]>();
  const currentSources = deferred<Source[]>();
  const requests: {
    recording: string;
    purpose: string;
    signal: AbortSignal;
  }[] = [];
  let nextReads = 0;
  const resolve: SourceResolver = async (recording, options) => {
    requests.push({
      recording,
      purpose: options.purpose,
      signal: options.signal,
    });
    if (options.purpose !== "cached")
      throw new Error(
        "Ready authorized audio must not start provider matching",
      );
    if (recording === "first") return [{ id: "first-audio", ready: true }];
    if (recording !== "next") throw new Error("Unexpected queue recording");
    nextReads++;
    return nextReads === 1 ? nextSources.promise : currentSources.promise;
  };
  const player = new Playback(initial, resolve, undefined, owner);
  let transition: Promise<void> | undefined;
  const played = () => ports.flatMap((media) => media.played);

  try {
    await player.playQueue([track("first"), track("next")]);
    await vi.waitFor(() => expect(nextReads).toBe(1));
    const entries = await player.queuePage(0, 10);
    expect(player.queueSnapshot().currentEntry).toBe(entries.entries[0].id);
    expect(played()).toEqual([path("first-audio")]);
    const originalLoads = [...initial.loads];
    const originalPauses = initial.pauses;
    initial.currentTime = 4;
    initial.dispatchEvent(new Event("timeupdate"));
    expect(initial.paused).toBe(false);
    expect(player.positionSnapshot().position).toBe(4);

    nextSources.resolve([{ id: "previous-choice", ready: true }]);
    const pending = await buffered.promise;
    pending.dispatchEvent(new Event("loadedmetadata"));
    pending.dispatchEvent(new Event("canplay"));
    expect(pending.paused).toBe(true);
    expect(pending.played).toEqual([]);
    expect(initial.src).toBe(path("first-audio"));
    expect(initial.pauses).toBe(originalPauses);
    expect(initial.loads).toEqual(originalLoads);
    initial.currentTime = 5;
    initial.dispatchEvent(new Event("timeupdate"));
    expect(player.snapshot().current?.id).toBe("first");
    expect(player.positionSnapshot().position).toBe(5);
    expect(player.snapshot().playing).toBe(true);

    initial.currentTime = initial.duration;
    initial.ended = true;
    initial.paused = true;
    transition = player.next(true);
    await vi.waitFor(() =>
      expect(
        nextReads === 2 || played().includes(path("previous-choice")),
      ).toBe(true),
    );
    expect(played()).not.toContain(path("previous-choice"));
    expect(nextReads).toBe(2);
    expect(played()).toEqual([path("first-audio")]);
    currentSources.resolve([
      { id: "previous-choice", ready: false },
      { id: "current-choice", ready: true },
    ]);
    await transition;

    expect(played()).toEqual([path("first-audio"), path("current-choice")]);
    expect(pending.played).toEqual([]);
    expect(pending.src).toBe("");
    expect(owned.has(pending)).toBe(false);
    expect(player.snapshot().current?.id).toBe("next");
    expect(player.snapshot().playing).toBe(true);
    expect(player.snapshot().error).toBe("");
    expect(player.queueSnapshot().currentEntry).toBe(entries.entries[1].id);
    expect((await player.queuePage(0, 10)).entries).toEqual(entries.entries);
    expect(
      requests.map(({ recording, purpose }) => `${recording}:${purpose}`),
    ).toEqual(["first:cached", "next:cached", "next:cached"]);
    expect(unexpectedNetwork).not.toHaveBeenCalled();
  } finally {
    nextSources.resolve([]);
    currentSources.resolve([]);
    player.clear();
    await transition?.catch(() => {});
    expect(requests.every((request) => request.signal.aborted)).toBe(true);
    expect(owned.size).toBe(1);
    expect([...owned].every((media) => media.paused && media.src === "")).toBe(
      true,
    );
  }
});

test("clearing a queue aborts held next-entry acquisition and rejects its late acknowledgement", async () => {
  const initial = new Media();
  const { ports, owned, owner, unexpectedNetwork } = mediaOwner(initial);
  const pending = deferred<Source[]>();
  const entered = deferred<AbortSignal>();
  const requests: {
    recording: string;
    purpose: string;
    signal: AbortSignal;
  }[] = [];
  const resolve: SourceResolver = async (recording, options) => {
    requests.push({
      recording,
      purpose: options.purpose,
      signal: options.signal,
    });
    if (options.purpose !== "cached")
      throw new Error("Ready fixture sources do not need matching");
    if (recording === "first") return [{ id: "first-audio", ready: true }];
    if (recording !== "next") throw new Error("Unexpected queue recording");
    entered.resolve(options.signal);
    return pending.promise;
  };
  const player = new Playback(initial, resolve, undefined, owner);

  try {
    await player.playQueue([track("first"), track("next")]);
    const signal = await entered.promise;
    expect(signal.aborted).toBe(false);
    expect(initial.src).toBe(path("first-audio"));
    expect(initial.paused).toBe(false);
    expect(player.snapshot().current?.id).toBe("first");
    initial.currentTime = 3;
    initial.dispatchEvent(new Event("timeupdate"));
    expect(player.positionSnapshot().position).toBe(3);

    player.clear();
    expect(signal.aborted).toBe(true);
    expect(initial.paused).toBe(true);
    expect(initial.src).toBe("");
    expect(player.snapshot().current).toBeNull();
    expect(player.snapshot().playing).toBe(false);
    expect(player.queueSnapshot().total).toBe(0);
    expect(player.queueSnapshot().currentEntry).toBeNull();

    pending.resolve([{ id: "late-next-audio", ready: true }]);
    await flushTasks();
    expect(owner.create).not.toHaveBeenCalled();
    expect(ports).toHaveLength(1);
    expect(ports.flatMap((media) => media.played)).toEqual([
      path("first-audio"),
    ]);
    expect(initial.loads).not.toContain(path("late-next-audio"));
    expect((await player.queuePage(0, 10)).entries).toEqual([]);
    expect(player.snapshot().current).toBeNull();
    expect(player.snapshot().playing).toBe(false);
    expect(player.snapshot().error).toBe("");
    expect(
      requests.map(({ recording, purpose }) => `${recording}:${purpose}`),
    ).toEqual(["first:cached", "next:cached"]);
    expect(unexpectedNetwork).not.toHaveBeenCalled();
  } finally {
    pending.resolve([]);
    player.clear();
    await flushTasks();
    expect(requests.every((request) => request.signal.aborted)).toBe(true);
    expect(owned.size).toBe(1);
    expect([...owned].every((media) => media.paused && media.src === "")).toBe(
      true,
    );
  }
});

test("removing a buffered next occurrence prepares its actual successor without interrupting current audio", async () => {
  const initial = new Media();
  const bufferedB = deferred<Media>();
  const bufferedC = deferred<Media>();
  const { ports, owned, owner, unexpectedNetwork } = mediaOwner(
    initial,
    (media) => {
      if (media.src === path("removed-audio")) bufferedB.resolve(media);
      if (media.src === path("successor-audio")) bufferedC.resolve(media);
    },
  );
  const bSources = deferred<Source[]>();
  const cSources = deferred<Source[]>();
  const requests: {
    recording: string;
    purpose: string;
    signal: AbortSignal;
  }[] = [];
  let cReads = 0;
  const resolve: SourceResolver = async (recording, options) => {
    requests.push({
      recording,
      purpose: options.purpose,
      signal: options.signal,
    });
    if (options.purpose !== "cached")
      throw new Error("Ready fixture sources do not need matching");
    if (recording === "first") return [{ id: "first-audio", ready: true }];
    if (recording === "removed") return bSources.promise;
    if (recording !== "successor")
      throw new Error("Unexpected queue recording");
    cReads++;
    return cReads === 1
      ? cSources.promise
      : [{ id: "successor-audio", ready: true }];
  };
  const player = new Playback(initial, resolve, undefined, owner);

  try {
    await player.playQueue([
      track("first"),
      track("removed"),
      track("successor"),
    ]);
    await vi.waitFor(() => expect(requests).toHaveLength(2));
    const before = await player.queuePage(0, 10);
    const originalLoads = [...initial.loads];
    const originalPauses = initial.pauses;
    bSources.resolve([{ id: "removed-audio", ready: true }]);
    const b = await bufferedB.promise;
    b.dispatchEvent(new Event("loadedmetadata"));
    b.dispatchEvent(new Event("canplay"));
    await flushTasks();
    expect(b.paused).toBe(true);
    expect(b.played).toEqual([]);
    initial.currentTime = 12.5;
    initial.dispatchEvent(new Event("timeupdate"));
    expect(player.snapshot().playing).toBe(true);

    player.removeEntry(before.entries[1].id);
    expect(requests[1].signal.aborted).toBe(true);
    expect(b.paused).toBe(true);
    expect(b.src).toBe("");
    expect(owned.has(b)).toBe(false);
    await vi.waitFor(() => expect(cReads).toBe(1));
    expect(initial.src).toBe(path("first-audio"));
    expect(initial.paused).toBe(false);
    expect(initial.currentTime).toBe(12.5);
    expect(initial.pauses).toBe(originalPauses);
    expect(initial.loads).toEqual(originalLoads);
    expect(player.queueSnapshot().currentEntry).toBe(before.entries[0].id);
    expect((await player.queuePage(0, 10)).entries).toEqual([
      before.entries[0],
      before.entries[2],
    ]);

    cSources.resolve([{ id: "successor-audio", ready: true }]);
    const c = await bufferedC.promise;
    c.dispatchEvent(new Event("loadedmetadata"));
    c.dispatchEvent(new Event("canplay"));
    initial.currentTime = 13.5;
    initial.dispatchEvent(new Event("timeupdate"));
    expect(player.positionSnapshot().position).toBe(13.5);
    expect(initial.pauses).toBe(originalPauses);
    expect(initial.loads).toEqual(originalLoads);
    expect(ports.flatMap((media) => media.played)).toEqual([
      path("first-audio"),
    ]);

    initial.currentTime = initial.duration;
    initial.ended = true;
    initial.paused = true;
    await player.next(true);
    expect(ports.flatMap((media) => media.played)).toEqual([
      path("first-audio"),
      path("successor-audio"),
    ]);
    expect(b.played).toEqual([]);
    expect(player.snapshot().current?.id).toBe("successor");
    expect(player.snapshot().playing).toBe(true);
    expect(player.snapshot().error).toBe("");
    expect(player.queueSnapshot().currentEntry).toBe(before.entries[2].id);
    expect((await player.queuePage(0, 10)).entries).toEqual([
      before.entries[0],
      before.entries[2],
    ]);
    expect(
      requests.map(({ recording, purpose }) => `${recording}:${purpose}`),
    ).toEqual([
      "first:cached",
      "removed:cached",
      "successor:cached",
      "successor:cached",
    ]);
    expect(unexpectedNetwork).not.toHaveBeenCalled();
  } finally {
    bSources.resolve([]);
    cSources.resolve([]);
    player.clear();
    await flushTasks();
    expect(requests.every((request) => request.signal.aborted)).toBe(true);
    expect(owned.size).toBe(1);
    expect([...owned].every((media) => media.paused && media.src === "")).toBe(
      true,
    );
  }
});

test("an exhausted next-entry attempt uses newly ready authorized audio at completion without matching again", async () => {
  const initial = new Media();
  const { ports, owned, owner, unexpectedNetwork } = mediaOwner(initial);
  const resolving = deferred<void>();
  const resolution = deferred<Source[]>();
  const requests: {
    recording: string;
    purpose: string;
    signal: AbortSignal;
  }[] = [];
  const resolutionIds: string[] = [];
  let available = false;
  const resolve: SourceResolver = async (recording, options) => {
    requests.push({
      recording,
      purpose: options.purpose,
      signal: options.signal,
    });
    if (recording === "first" && options.purpose === "cached")
      return [{ id: "first-audio", ready: true }];
    if (recording !== "next") throw new Error("Unexpected queue recording");
    if (options.purpose === "cached")
      return available ? [{ id: "later-ready-audio", ready: true }] : [];
    if (options.purpose === "refresh") return [];
    if (options.purpose === "resolve")
      resolutionIds.push(options.body.request_id);
    resolving.resolve();
    return resolution.promise;
  };
  const player = new Playback(initial, resolve, undefined, owner);

  try {
    await player.playQueue([track("first"), track("next")]);
    await resolving.promise;
    const entries = await player.queuePage(0, 10);
    const originalLoads = [...initial.loads];
    const originalPauses = initial.pauses;
    initial.currentTime = 7;
    initial.dispatchEvent(new Event("timeupdate"));
    expect(initial.paused).toBe(false);
    expect(player.snapshot().current?.id).toBe("first");
    expect(player.positionSnapshot().position).toBe(7);
    expect(resolutionIds).toHaveLength(1);
    expect(resolutionIds[0]).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i,
    );

    resolution.resolve([]);
    await flushTasks();
    expect(owner.create).not.toHaveBeenCalled();
    expect(player.snapshot().playing).toBe(true);
    expect(player.snapshot().error).toBe("");
    available = true;
    initial.currentTime = 12.5;
    initial.dispatchEvent(new Event("timeupdate"));
    expect(initial.src).toBe(path("first-audio"));
    expect(initial.pauses).toBe(originalPauses);
    expect(initial.loads).toEqual(originalLoads);
    expect(player.positionSnapshot().position).toBe(12.5);

    initial.currentTime = initial.duration;
    initial.ended = true;
    initial.paused = true;
    await player.next(true);
    expect(ports.flatMap((media) => media.played)).toEqual([
      path("first-audio"),
      path("later-ready-audio"),
    ]);
    expect(player.snapshot().current?.id).toBe("next");
    expect(player.snapshot().playing).toBe(true);
    expect(player.snapshot().error).toBe("");
    expect(player.queueSnapshot().currentEntry).toBe(entries.entries[1].id);
    expect((await player.queuePage(0, 10)).entries).toEqual(entries.entries);
    expect(
      requests.map(({ recording, purpose }) => `${recording}:${purpose}`),
    ).toEqual([
      "first:cached",
      "next:cached",
      "next:refresh",
      "next:resolve",
      "next:cached",
    ]);
    expect(resolutionIds).toHaveLength(1);
    expect(unexpectedNetwork).not.toHaveBeenCalled();
  } finally {
    resolution.resolve([]);
    player.clear();
    await flushTasks();
    expect(requests.every((request) => request.signal.aborted)).toBe(true);
    expect(owned.size).toBe(1);
    expect([...owned].every((media) => media.paused && media.src === "")).toBe(
      true,
    );
  }
});

test("a failed pending seek during buffered handoff recovers through authorized backup audio without playing the failed source", async () => {
  class FailedSeekMedia extends Media {
    failedPositions: number[] = [];
    override get currentTime() {
      return super.currentTime;
    }
    override set currentTime(value: number) {
      if (value > 0 && this.src === path("prepared-audio")) {
        this.failedPositions.push(value);
        throw new DOMException(
          "Owned buffered seek failed",
          "InvalidStateError",
        );
      }
      super.currentTime = value;
    }
  }
  const initial = new Media();
  const buffered = deferred<FailedSeekMedia>();
  const { ports, owned, owner, unexpectedNetwork } = mediaOwner(
    initial,
    (media) => {
      if (
        media instanceof FailedSeekMedia &&
        media.src === path("prepared-audio")
      )
        buffered.resolve(media);
    },
    () => new FailedSeekMedia(),
  );
  const checking = deferred<void>();
  const authority = deferred<Source[]>();
  const requests: {
    recording: string;
    purpose: string;
    signal: AbortSignal;
  }[] = [];
  let nextReads = 0;
  const ready: Source[] = [
    { id: "prepared-audio", ready: true },
    { id: "backup-audio", ready: true },
  ];
  const resolve: SourceResolver = async (recording, options) => {
    requests.push({
      recording,
      purpose: options.purpose,
      signal: options.signal,
    });
    if (options.purpose !== "cached")
      throw new Error("Ready backup audio must not repeat provider matching");
    if (recording === "first") return [{ id: "first-audio", ready: true }];
    if (recording !== "next") throw new Error("Unexpected queue recording");
    nextReads++;
    if (nextReads === 1) return ready;
    checking.resolve();
    return authority.promise;
  };
  const player = new Playback(initial, resolve, undefined, owner);
  let transition: Promise<void> | undefined;

  try {
    await player.playQueue([track("first"), track("next")]);
    const entries = await player.queuePage(0, 10);
    const b = await buffered.promise;
    b.dispatchEvent(new Event("loadedmetadata"));
    b.dispatchEvent(new Event("canplay"));
    expect(b.paused).toBe(true);
    expect(b.played).toEqual([]);

    initial.currentTime = initial.duration;
    initial.ended = true;
    initial.paused = true;
    transition = player.next(true);
    await checking.promise;
    player.seek(30);
    expect(player.snapshot().current?.id).toBe("next");
    expect(player.snapshot().loading).toBe(true);
    expect(player.positionSnapshot().position).toBe(30);
    expect(b.failedPositions).toEqual([]);
    expect(ports.flatMap((media) => media.played)).toEqual([
      path("first-audio"),
    ]);

    authority.resolve(ready);
    await transition;
    await flushTasks();
    expect(b.failedPositions).toEqual([30]);
    expect(b.played).not.toContain(path("prepared-audio"));
    await vi.waitFor(() =>
      expect(ports.flatMap((media) => media.played)).toEqual([
        path("first-audio"),
        path("backup-audio"),
      ]),
    );
    const backup = ports.find((media) => media.src === path("backup-audio"));
    expect(backup).toBeDefined();
    backup!.dispatchEvent(new Event("loadedmetadata"));
    expect(backup!.currentTime).toBe(30);
    expect(player.positionSnapshot().position).toBe(30);
    expect(player.snapshot().current?.id).toBe("next");
    expect(player.snapshot().playing).toBe(true);
    expect(player.snapshot().error).toBe("");
    expect(player.queueSnapshot().currentEntry).toBe(entries.entries[1].id);
    expect((await player.queuePage(0, 10)).entries).toEqual(entries.entries);
    expect(
      requests.map(({ recording, purpose }) => `${recording}:${purpose}`),
    ).toEqual(["first:cached", "next:cached", "next:cached"]);
    expect(unexpectedNetwork).not.toHaveBeenCalled();
  } finally {
    authority.resolve([]);
    player.clear();
    await transition?.catch(() => {});
    await flushTasks();
    expect(requests.every((request) => request.signal.aborted)).toBe(true);
    expect(owned.size).toBe(1);
    expect([...owned].every((media) => media.paused && media.src === "")).toBe(
      true,
    );
  }
});
