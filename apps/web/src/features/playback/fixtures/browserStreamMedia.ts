import type { MediaPort } from "../model";

export class StreamMedia extends EventTarget implements MediaPort {
  src = "";
  currentTime = 0;
  duration = 90;
  readyState = 0;
  playbackRate = 1;
  defaultPlaybackRate = 1;
  volume = 1;
  paused = true;
  seeking = false;
  ended = false;
  error: { code: number } | null = null;
  async play() {
    this.ended = false;
    this.paused = false;
    this.dispatchEvent(new Event("play"));
  }
  pause() {
    if (this.paused) return;
    this.paused = true;
    this.dispatchEvent(new Event("pause"));
  }
  load() {
    this.currentTime = 0;
    this.readyState = 0;
    this.error = null;
    this.dispatchEvent(new Event("loadstart"));
  }
  ready(duration = 90) {
    this.duration = duration;
    this.readyState = 4;
    this.dispatchEvent(new Event("loadedmetadata"));
    this.dispatchEvent(new Event("canplay"));
  }
  advance(position: number) {
    this.currentTime = position;
    this.dispatchEvent(new Event("timeupdate"));
  }
  removeAttribute() {
    this.src = "";
  }
}

export const streamTrack = {
  id: "recording",
  title: "Owned fixture",
  artist: "Fixture",
  duration_ms: 90000,
  available: true,
};

export function convertedHead(input: RequestInfo | URL) {
  const url = new URL(String(input), "http://hito.invalid");
  return new Response(null, {
    headers: {
      "X-Hito-Stream-Quality": url.searchParams.get("quality") ?? "original",
      "X-Hito-Stream-Start-Ms": url.searchParams.get("start_ms") ?? "0",
    },
  });
}
