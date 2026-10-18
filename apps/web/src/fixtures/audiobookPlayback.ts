import { vi } from "vitest";
import { Playback, type MediaPort } from "../playback";

export class AudiobookMedia extends EventTarget implements MediaPort {
  src = "";
  currentTime = 0;
  duration = 120;
  playbackRate = 1;
  defaultPlaybackRate = 1;
  volume = 1;
  paused = true;
  async play() {
    this.paused = false;
    this.dispatchEvent(new Event("play"));
  }
  pause() {
    this.paused = true;
    this.dispatchEvent(new Event("pause"));
  }
  load() {}
  removeAttribute() {
    this.src = "";
  }
}

export function audiobookPlayer() {
  const media = new AudiobookMedia();
  const resolve = vi.fn(async () => [{ id: "owned-source", ready: true }]);
  const progress = vi.fn();
  const player = new Playback(media, resolve, progress);
  return { media, resolve, progress, player };
}
