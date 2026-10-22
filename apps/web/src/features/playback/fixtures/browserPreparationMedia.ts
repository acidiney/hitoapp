import { vi } from "vitest";

export function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((finish) => {
    resolve = finish;
  });
  return { promise, resolve };
}

export const audioPath = (source: string) => `/api/v1/sources/${source}/audio`;

export function browserPreparationMedia() {
  const created: HTMLAudioElement[] = [];
  const played: { media: HTMLMediaElement; source: string | null }[] = [];
  const loaded: { media: HTMLAudioElement; source: string | null }[] = [];
  const states = new WeakMap<
    HTMLMediaElement,
    { paused: boolean; ended: boolean }
  >();
  const waiting = new Map<string, (media: HTMLAudioElement) => void>();
  const owned = new Set<HTMLAudioElement>();
  let heldPlay: {
    media?: HTMLMediaElement;
    cancelled: boolean;
    entered: ReturnType<typeof deferred<HTMLMediaElement>>;
    gate: ReturnType<typeof deferred<void>>;
  } | null = null;
  let peak = 0;
  const recordOwnership = (records: MutationRecord[]) => {
    for (const record of records) {
      for (const node of record.removedNodes)
        if (node instanceof HTMLAudioElement) owned.delete(node);
      for (const node of record.addedNodes)
        if (node instanceof HTMLAudioElement && created.includes(node))
          owned.add(node);
      peak = Math.max(peak, owned.size);
    }
  };
  const observer = new MutationObserver(recordOwnership);
  observer.observe(document.body, { childList: true });
  const state = (media: HTMLMediaElement) => {
    let value = states.get(media);
    if (!value) {
      value = { paused: true, ended: false };
      states.set(media, value);
    }
    return value;
  };
  vi.stubGlobal("Audio", function () {
    const media = document.createElement("audio");
    created.push(media);
    return media;
  });
  vi.spyOn(HTMLMediaElement.prototype, "paused", "get").mockImplementation(
    function (this: HTMLMediaElement) {
      return state(this).paused;
    },
  );
  vi.spyOn(HTMLMediaElement.prototype, "ended", "get").mockImplementation(
    function (this: HTMLMediaElement) {
      return state(this).ended;
    },
  );
  vi.spyOn(HTMLMediaElement.prototype, "duration", "get").mockReturnValue(90);
  vi.spyOn(HTMLMediaElement.prototype, "play").mockImplementation(
    async function (this: HTMLMediaElement) {
      played.push({ media: this, source: this.getAttribute("src") });
      state(this).paused = false;
      state(this).ended = false;
      const held = heldPlay;
      if (held && !held.media) {
        held.media = this;
        held.entered.resolve(this);
        await held.gate.promise;
        if (heldPlay === held) heldPlay = null;
        if (held.cancelled)
          throw new DOMException(
            "Owned media play was interrupted",
            "AbortError",
          );
      }
      this.dispatchEvent(new Event("play"));
    },
  );
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(function (
    this: HTMLMediaElement,
  ) {
    const playing = !state(this).paused;
    state(this).paused = true;
    if (heldPlay?.media === this) heldPlay.cancelled = true;
    if (playing) this.dispatchEvent(new Event("pause"));
  });
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(function (
    this: HTMLAudioElement,
  ) {
    const source = this.getAttribute("src");
    loaded.push({ media: this, source });
    this.currentTime = 0;
    state(this).ended = false;
    this.dispatchEvent(new Event("loadstart"));
    if (source) waiting.get(source)?.(this);
  });

  return {
    created,
    played,
    loaded,
    holdNextPlay() {
      if (heldPlay) throw new Error("Only one owned play boundary may be held");
      const held = {
        cancelled: false,
        entered: deferred<HTMLMediaElement>(),
        gate: deferred<void>(),
      };
      heldPlay = held;
      return {
        entered: held.entered.promise,
        release: () => held.gate.resolve(),
      };
    },
    loadedSource(source: string) {
      const path = audioPath(source);
      const existing = loaded.find((load) => load.source === path);
      if (existing) return Promise.resolve(existing.media);
      const next = deferred<HTMLAudioElement>();
      waiting.set(path, next.resolve);
      return next.promise;
    },
    canPlay(media: HTMLAudioElement) {
      media.dispatchEvent(new Event("loadedmetadata"));
      media.dispatchEvent(new Event("canplay"));
    },
    end(media: HTMLAudioElement) {
      media.currentTime = media.duration;
      state(media).paused = true;
      state(media).ended = true;
      media.dispatchEvent(new Event("ended"));
    },
    peakOwned() {
      recordOwnership(observer.takeRecords());
      return peak;
    },
    close() {
      observer.disconnect();
    },
  };
}
