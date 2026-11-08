import { expect, vi } from "vitest";
import type { MediaPort, Track } from "../playback";
import type { BrowserMediaOwner } from "../browser/BrowserPreparation";

export class QueueMedia extends EventTarget implements MediaPort {
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

  async play() {
    this.played.push(this.src);
    this.paused = false;
    this.dispatchEvent(new Event("play"));
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
  }
  removeAttribute(name: string) {
    if (name === "src") this.src = "";
  }
}

export const queueTrack = (id: string, title = id): Track => ({
  id,
  title,
  artist: "Owned performer",
  duration_ms: 90000,
  available: true,
});

export function queueMediaOwner(initial: QueueMedia) {
  const owned = new Set<MediaPort>([initial]);
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => {
      throw new Error("Queue edits must not create HTTP work");
    }),
  );
  const owner: BrowserMediaOwner = {
    create: vi.fn(() => {
      const media = new QueueMedia();
      owned.add(media);
      return media;
    }),
    release: (media) => {
      owned.delete(media);
    },
  };
  return owner;
}
