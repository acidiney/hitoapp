import { setImmediate as flushTasks } from "node:timers/promises";
import { expect, vi } from "vitest";
import { Playback, type MediaPort, type Track } from "../playback";
import type { SourceResolver } from "../api/playbackSources";
import type { BrowserMediaOwner } from "../browser/BrowserPreparation";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((finish) => {
    resolve = finish;
  });
  return { promise, resolve };
}

export const audioPath = (source: string) =>
  `/api/v1/sources/${encodeURIComponent(source)}/audio`;

export const recording = (id: string): Track => ({
  id,
  title: "Owned recording",
  artist: "Owned performer",
  duration_ms: 90000,
  available: true,
});

class Media extends EventTarget implements MediaPort {
  src = "";
  currentTime = 0;
  duration = 90;
  playbackRate = 1;
  defaultPlaybackRate = 1;
  volume = 1;
  paused = true;
  ended = false;
  loads: string[] = [];
  played: string[] = [];
  private started = deferred<void>();
  readonly playing = this.started.promise;

  constructor(private loaded: (media: Media) => void = () => {}) {
    super();
  }

  async play() {
    this.played.push(this.src);
    this.paused = false;
    this.ended = false;
    this.dispatchEvent(new Event("play"));
    this.started.resolve();
  }

  pause() {
    this.paused = true;
    this.dispatchEvent(new Event("pause"));
  }

  load() {
    this.loads.push(this.src);
    this.currentTime = 0;
    this.ended = false;
    this.dispatchEvent(new Event("loadstart"));
    this.loaded(this);
  }

  removeAttribute(name: string) {
    if (name === "src") this.src = "";
  }

  ready() {
    this.dispatchEvent(new Event("loadedmetadata"));
    this.dispatchEvent(new Event("canplay"));
  }

  complete() {
    this.currentTime = this.duration;
    this.ended = true;
    this.paused = true;
    this.dispatchEvent(new Event("ended"));
  }
}

export function browserPreparationModes(
  resolve: SourceResolver = async (id) => [{ id: `${id}-audio`, ready: true }],
) {
  const initial = new Media();
  const ports = [initial];
  const owned = new Set<MediaPort>(ports);
  const released: MediaPort[] = [];
  const waiting: { path: string; resolve: (media: Media) => void }[] = [];
  const requests: {
    recording: string;
    purpose: Parameters<SourceResolver>[1]["purpose"];
    signal: AbortSignal;
  }[] = [];
  let peakOwned = 1;
  const unexpectedNetwork = vi.fn(async () => {
    throw new Error("Playback must not create downloads or other HTTP work");
  });
  vi.stubGlobal("fetch", unexpectedNetwork);

  const owner: BrowserMediaOwner = {
    create: () => {
      const media = new Media((loaded) => {
        for (const pending of waiting)
          if (pending.path === loaded.src) pending.resolve(loaded);
      });
      ports.push(media);
      owned.add(media);
      peakOwned = Math.max(peakOwned, owned.size);
      expect(owned.size).toBeLessThanOrEqual(2);
      return media;
    },
    release: (media) => {
      expect(media.paused).toBe(true);
      expect(media.src).toBe("");
      expect(owned.delete(media)).toBe(true);
      released.push(media);
    },
  };
  const player = new Playback(
    initial,
    async (id, options) => {
      requests.push({
        recording: id,
        purpose: options.purpose,
        signal: options.signal,
      });
      return resolve(id, options);
    },
    undefined,
    owner,
  );

  return {
    player,
    initial,
    owned,
    released,
    requests,
    async buffer(source: string) {
      const path = audioPath(source);
      const loaded = ports.find(
        (media) =>
          media !== initial && media.src === path && !media.played.length,
      );
      if (loaded) return loaded;
      const pending = deferred<Media>();
      waiting.push({ path, resolve: pending.resolve });
      return pending.promise;
    },
    async close() {
      player.clear();
      await flushTasks();
      expect(requests.every(({ signal }) => signal.aborted)).toBe(true);
      expect(peakOwned).toBeLessThanOrEqual(2);
      expect([...owned]).toEqual([initial]);
      expect(ports.every((media) => media.paused && media.src === "")).toBe(
        true,
      );
      expect(new Set(released)).toEqual(
        new Set(ports.filter((media) => media !== initial)),
      );
      expect(unexpectedNetwork).not.toHaveBeenCalled();
    },
  };
}
