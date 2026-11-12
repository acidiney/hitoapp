import type { MediaPort } from "../model";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import { streamingQuality, type StreamingQuality } from "./StreamingQuality";

export function sourceAudioUrl(source: string) {
  return `/api/v1/sources/${encodeURIComponent(source)}/audio`;
}

const EVENTS = [
  "loadedmetadata",
  "durationchange",
  "timeupdate",
  "play",
  "playing",
  "seeked",
  "seeking",
  "pause",
  "error",
  "ended",
  "canplay",
  "stalled",
  "waiting",
  "ratechange",
  "loadstart",
] as const;

export class BrowserStream extends EventTarget implements MediaPort {
  private source = "";
  private offset = 0;
  private fullDuration = 0;
  private converted = false;
  private desiredPlaying = false;
  private relocating = false;
  private replacing = false;
  private originalStart: number | null = null;
  private triedOriginal = false;
  private operation = createUuid();
  private controller: AbortController | null = null;
  private opening: Promise<void> | null = null;
  private revision = 0;
  private forwarding: [string, EventListener][] = [];

  constructor(readonly original: MediaPort) {
    super();
  }

  activate() {
    if (this.forwarding.length > 0) return;
    for (const name of EVENTS) {
      const listener = () => this.forward(name);
      this.original.addEventListener(name, listener);
      this.forwarding.push([name, listener]);
    }
  }

  private forward(name: string) {
    if (this.opening || this.replacing) return;
    if (
      (name === "loadedmetadata" || name === "durationchange") &&
      this.converted &&
      this.fullDuration <= 0 &&
      this.offset === 0 &&
      Number.isFinite(this.original.duration) &&
      this.original.duration > 0
    )
      this.fullDuration = this.original.duration;
    if (name === "error" && this.fallback()) {
      if (this.desiredPlaying) this.resume();
      return;
    }
    const revision = this.revision;
    const relocated = name === "loadedmetadata" && this.relocating;
    if (name === "loadedmetadata" && this.originalStart !== null) {
      const position = this.originalStart;
      try {
        this.original.currentTime = position;
        this.originalStart = null;
      } catch {
        log.debug("playback.stream_failed", {
          operation_id: this.operation,
          generation: revision,
          reason: "original_seek_failed",
        });
        this.dispatchEvent(new Event("error"));
        return;
      }
    }
    if (relocated || name === "seeked") this.relocating = false;
    this.dispatchEvent(new Event(name));
    if (relocated && revision === this.revision && !this.original.seeking)
      this.dispatchEvent(new Event("seeked"));
  }

  openSource(source: string, start: number, duration: number) {
    this.activate();
    this.source = source;
    this.fullDuration = duration;
    this.triedOriginal = false;
    this.open(start, streamingQuality());
  }

  private open(start: number, quality: StreamingQuality) {
    const revision = ++this.revision;
    this.controller?.abort();
    this.controller = null;
    this.opening = null;
    this.offset = 0;
    this.converted = false;
    this.originalStart = null;
    const path = sourceAudioUrl(this.source);
    if (quality === "original") {
      this.replace(path, start, false);
      return;
    }
    const controller = new AbortController();
    this.controller = controller;
    const startMs = Math.round(Math.max(0, start) * 1000);
    const url = `${path}?quality=${quality}&start_ms=${startMs}`;
    this.offset = startMs / 1000;
    const started = performance.now();
    this.operation = createUuid();
    const loading = this.negotiate(url, quality, controller);
    this.opening = loading
      .then((converted) => {
        if (revision !== this.revision) return;
        this.opening = null;
        this.replace(converted ? url : path, startMs / 1000, converted);
        log.debug("playback.stream_opened", {
          operation_id: this.operation,
          generation: revision,
          quality,
          outcome: converted ? "converted" : "original",
          duration_ms: Math.round(performance.now() - started),
        });
      })
      .catch(() => {
        if (revision !== this.revision) return;
        this.opening = null;
        this.triedOriginal = true;
        this.replace(path, startMs / 1000, false);
        log.debug("playback.stream_fallback", {
          operation_id: this.operation,
          generation: revision,
          quality,
          reason: "negotiation_failed",
          duration_ms: Math.round(performance.now() - started),
        });
      });
    this.original.pause();
    this.original.removeAttribute("src");
    this.original.load();
  }

  private replace(url: string, start: number, converted: boolean) {
    this.converted = converted;
    this.offset = converted ? start : 0;
    this.originalStart = !converted && start > 0 ? start : null;
    this.original.src = url;
    this.original.load();
  }

  private async negotiate(
    url: string,
    quality: StreamingQuality,
    controller: AbortController,
  ) {
    const timer = setTimeout(() => controller.abort(), 15_000);
    try {
      const response = await fetch(url, {
        method: "HEAD",
        credentials: "same-origin",
        cache: "no-store",
        signal: controller.signal,
      });
      if (!response.ok) throw new Error("Audio quality could not be checked.");
      const converted =
        response.headers.get("X-Hito-Stream-Quality") === quality;
      if (
        converted &&
        response.headers.get("X-Hito-Stream-Start-Ms") !==
          new URL(url, "http://hito.invalid").searchParams.get("start_ms")
      )
        throw new Error("Audio quality response did not match the request.");
      return converted;
    } finally {
      clearTimeout(timer);
    }
  }

  private fallback() {
    if (!this.converted || this.triedOriginal) return false;
    const position = this.currentTime;
    this.triedOriginal = true;
    this.relocating = true;
    this.dispatchEvent(new Event("seeking"));
    this.replacing = true;
    this.original.pause();
    this.replacing = false;
    this.open(position, "original");
    log.debug("playback.stream_fallback", {
      operation_id: this.operation,
      generation: this.revision,
      reason: "converted_audio_failed",
      outcome: "original",
    });
    return true;
  }

  private resume() {
    void this.play().catch(() => this.dispatchEvent(new Event("error")));
  }

  get src() {
    return this.original.src;
  }
  set src(value: string) {
    this.original.src = value;
  }
  get currentSrc() {
    return this.original.currentSrc;
  }
  get currentTime() {
    if (this.opening) return this.offset;
    if (this.originalStart !== null) return this.originalStart;
    return this.original.currentTime + (this.converted ? this.offset : 0);
  }
  set currentTime(position: number) {
    if (this.converted && Math.abs(position - this.currentTime) >= 0.001) {
      const playing = this.desiredPlaying || !this.original.paused;
      this.relocating = true;
      this.dispatchEvent(new Event("seeking"));
      this.triedOriginal = false;
      this.open(position, streamingQuality());
      if (playing) this.resume();
    } else if (!this.converted) this.original.currentTime = position;
  }
  get duration() {
    return this.converted ? this.fullDuration : this.original.duration;
  }
  get paused() {
    return this.opening ? !this.desiredPlaying : this.original.paused;
  }
  get ended() {
    return this.original.ended;
  }
  get readyState() {
    return this.opening ? 0 : this.original.readyState;
  }
  get seeking() {
    return this.relocating || this.original.seeking;
  }
  get error() {
    return this.original.error;
  }
  get playbackRate() {
    return this.original.playbackRate;
  }
  set playbackRate(value: number) {
    this.original.playbackRate = value;
  }
  get defaultPlaybackRate() {
    return this.original.defaultPlaybackRate;
  }
  set defaultPlaybackRate(value: number) {
    this.original.defaultPlaybackRate = value;
  }
  get volume() {
    return this.original.volume;
  }
  set volume(value: number) {
    this.original.volume = value;
  }
  play(): Promise<void> {
    this.desiredPlaying = true;
    const revision = this.revision;
    const playing = this.opening
      ? this.opening.then(() => {
          if (revision === this.revision && this.desiredPlaying)
            return this.original.play();
        })
      : this.original.play();
    return playing.catch((error: unknown) => {
      if (revision !== this.revision) return;
      if (error instanceof Error && error.name === "NotAllowedError")
        throw error;
      if (this.fallback()) return this.play();
      throw error;
    });
  }
  pause() {
    const waiting = this.opening !== null;
    const playing = !this.paused;
    this.desiredPlaying = false;
    this.original.pause();
    if (waiting && playing) this.dispatchEvent(new Event("pause"));
  }
  load() {
    this.original.load();
  }
  removeAttribute(name: string) {
    if (name === "src") {
      this.revision++;
      this.controller?.abort();
      this.controller = null;
      this.opening = null;
      this.offset = 0;
      this.converted = false;
      this.relocating = false;
      this.originalStart = null;
    }
    this.original.removeAttribute(name);
  }
  close() {
    this.removeAttribute("src");
    for (const [name, listener] of this.forwarding)
      this.original.removeEventListener(name, listener);
    this.forwarding = [];
  }
}
