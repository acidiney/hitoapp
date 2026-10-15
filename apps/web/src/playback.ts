import type { Recording, Source as ApiSource } from "./types";
import { log } from "./log";

export type Track = Pick<
  Recording,
  "id" | "title" | "artist" | "duration_ms" | "available"
> &
  Partial<Pick<Recording, "inherited_title" | "inherited_artist">>;
export type Source = Pick<ApiSource, "id" | "ready">;
export interface MediaPort extends EventTarget {
  src: string;
  currentTime: number;
  duration: number;
  playbackRate: number;
  defaultPlaybackRate: number;
  volume: number;
  paused: boolean;
  play(): Promise<void>;
  pause(): void;
  load(): void;
  removeAttribute(name: string): void;
}
export type PlaybackState = {
  current: Track | null;
  queue: Track[];
  index: number;
  playing: boolean;
  loading: boolean;
  position: number;
  duration: number;
  error: string;
  repeat: "off" | "all" | "one";
  shuffle: boolean;
  speed: number;
};

export class Playback {
  private state: PlaybackState = {
    current: null,
    queue: [],
    index: -1,
    playing: false,
    loading: false,
    position: 0,
    duration: 0,
    error: "",
    repeat: "off",
    shuffle: false,
    speed: 1,
  };
  private listeners = new Set<() => void>();
  private sources: Source[] = [];
  private generation = 0;
  private selection = 0;
  private sourceAttempt = 0;
  private lastSavedPosition = 0;
  private pendingSeek: number | null = null;
  private activeSource = false;
  private failed = new Set<number>();

  constructor(
    private media: MediaPort,
    private resolve: (recording: string) => Promise<Source[]>,
    private onProgress?: (
      track: Track,
      position: number,
      speed: number,
    ) => void,
  ) {
    media.addEventListener("loadedmetadata", () => {
      log.debug("playback.metadata", { recording_id: this.state.current?.id });
      if (this.pendingSeek !== null) {
        media.currentTime = this.pendingSeek;
        this.pendingSeek = null;
      }
      this.update({
        duration: Number.isFinite(media.duration) ? media.duration : 0,
      });
    });
    media.addEventListener("timeupdate", () => {
      this.update({ position: media.currentTime });
      if (Math.abs(media.currentTime - this.lastSavedPosition) >= 10)
        this.persist();
    });
    media.addEventListener("play", () => {
      log.info("playback.started", { recording_id: this.state.current?.id });
      this.update({ playing: true, loading: false });
    });
    media.addEventListener("pause", () => {
      log.debug("playback.paused", { recording_id: this.state.current?.id });
      this.persist();
      this.update({ playing: false });
    });
    media.addEventListener("error", () => {
      log.warn("playback.media_failed", {
        recording_id: this.state.current?.id,
      });
      if (this.activeSource) void this.recover();
    });
    media.addEventListener("ended", () => {
      log.info("playback.ended", { recording_id: this.state.current?.id });
      void this.next(true);
    });
  }

  private persist() {
    if (
      this.state.current &&
      this.media.currentTime > 0 &&
      this.lastSavedPosition !== this.media.currentTime
    ) {
      this.lastSavedPosition = this.media.currentTime;
      log.debug("playback.position", {
        recording_id: this.state.current.id,
        position_ms: Math.floor(this.media.currentTime * 1000),
      });
      this.onProgress?.(
        this.state.current,
        this.media.currentTime,
        this.media.playbackRate,
      );
    }
  }

  snapshot = () => this.state;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  private update(patch: Partial<PlaybackState>) {
    this.state = { ...this.state, ...patch };
    this.listeners.forEach((listener) => listener());
  }

  setRecordingField(
    recording: string,
    field: "title" | "artist",
    personal: string | null,
    inherited: string,
  ) {
    const update = (track: Track): Track =>
      track.id === recording
        ? {
            ...track,
            [field]: personal ?? track[`inherited_${field}`] ?? inherited,
          }
        : track;
    log.info("playback.metadata_updated", {
      recording_id: recording,
      action: field,
    });
    this.update({
      queue: this.state.queue.map(update),
      current: this.state.current ? update(this.state.current) : null,
    });
  }

  async playQueue(queue: Track[], index = 0, position = 0) {
    log.info("queue.play", {
      count: queue.length,
      index,
      position_ms: Math.floor(position * 1000),
    });
    this.failed.clear();
    this.update({ queue: [...queue], index, error: "" });
    await this.open(index, position);
  }

  prepareSelection(): (track: Track) => Promise<boolean> {
    const selection = ++this.selection;
    log.debug("playback.selection_prepared");
    return async (track) => {
      if (selection !== this.selection) {
        log.debug("playback.selection_superseded");
        return false;
      }
      await this.playQueue([track]);
      return true;
    };
  }

  private async open(index: number, position = 0) {
    this.selection++;
    this.persist();
    this.lastSavedPosition = 0;
    const generation = ++this.generation;
    const current = this.state.queue[index];
    log.info("playback.resolving", { recording_id: current?.id, index });
    this.sources = [];
    this.media.pause();
    this.activeSource = false;
    this.media.removeAttribute("src");
    this.media.load();
    this.pendingSeek = position;
    this.update({
      current: current ?? null,
      index,
      position,
      loading: !!current,
      playing: false,
      duration: (current?.duration_ms ?? 0) / 1000,
    });
    if (!current) {
      this.update({ loading: false });
      return;
    }
    try {
      const sources = await this.resolve(current.id);
      if (generation !== this.generation) return;
      this.sources = sources.filter((source) => source.ready);
      log.debug("playback.resolved", {
        recording_id: current.id,
        count: this.sources.length,
      });
      await this.useNextSource(position);
    } catch {
      log.warn("playback.resolution_failed", { recording_id: current.id });
      if (generation === this.generation) await this.skipFailed();
    }
  }

  private async useNextSource(position: number) {
    const generation = this.generation;
    const attempt = ++this.sourceAttempt;
    const source = this.sources.shift();
    if (!source) {
      await this.skipFailed();
      return;
    }
    this.pendingSeek = position;
    this.activeSource = true;
    log.info("playback.source_changed", {
      recording_id: this.state.current?.id,
      source_id: source.id,
      position_ms: Math.floor(position * 1000),
    });
    this.media.src = `/api/v1/sources/${encodeURIComponent(source.id)}/audio`;
    this.media.load();
    try {
      await this.media.play();
    } catch (error) {
      if (generation !== this.generation || attempt !== this.sourceAttempt)
        return;
      if (error instanceof Error && error.name === "NotAllowedError")
        this.update({ loading: false, error: "Press play to start audio." });
      else await this.recover();
    }
  }

  async recover() {
    log.warn("playback.recovering", {
      recording_id: this.state.current?.id,
      count: this.sources.length,
    });
    await this.useNextSource(
      Math.max(this.pendingSeek ?? 0, this.media.currentTime),
    );
  }

  private async skipFailed() {
    log.warn("playback.unavailable", {
      recording_id: this.state.current?.id,
      index: this.state.index,
    });
    this.failed.add(this.state.index);
    this.update({
      error: `Audio is unavailable for ${this.state.current?.title ?? "this track"}.`,
      loading: false,
    });
    const next = this.state.queue.findIndex(
      (_, i) => i > this.state.index && !this.failed.has(i),
    );
    if (next >= 0) await this.open(next);
    else {
      this.media.pause();
      this.update({ playing: false, loading: false });
    }
  }

  async toggle() {
    this.selection++;
    const generation = this.generation;
    const attempt = ++this.sourceAttempt;
    log.info("playback.toggle", {
      recording_id: this.state.current?.id,
      enabled: this.media.paused,
    });
    if (this.media.paused) {
      try {
        await this.media.play();
      } catch {
        if (generation !== this.generation || attempt !== this.sourceAttempt)
          return;
        this.update({
          error: "Audio could not start. Select the track again.",
        });
      }
    } else this.media.pause();
  }
  seek(seconds: number) {
    this.media.currentTime = Math.max(
      0,
      Math.min(seconds, this.state.duration || seconds),
    );
    this.update({ position: this.media.currentTime });
    log.debug("playback.seek", {
      recording_id: this.state.current?.id,
      position_ms: Math.floor(this.media.currentTime * 1000),
    });
  }
  speed(value: number) {
    log.info("playback.speed", { speed: value });
    this.media.defaultPlaybackRate = value;
    this.media.playbackRate = value;
    this.update({ speed: value });
  }
  volume(value: number) {
    log.debug("playback.volume", { volume: value });
    this.media.volume = value;
  }
  repeat() {
    log.info("queue.repeat", { mode: this.state.repeat });
    this.update({
      repeat:
        this.state.repeat === "off"
          ? "all"
          : this.state.repeat === "all"
            ? "one"
            : "off",
    });
  }
  shuffle() {
    log.info("queue.shuffle", { enabled: !this.state.shuffle });
    this.update({ shuffle: !this.state.shuffle });
  }
  async previous() {
    this.selection++;
    log.info("queue.previous", { index: this.state.index });
    if (this.media.currentTime > 3) this.seek(0);
    else await this.open(Math.max(0, this.state.index - 1));
  }
  async next(ended = false) {
    log.info("queue.next", {
      index: this.state.index,
      reason: ended ? "ended" : "requested",
    });
    let index = this.state.index + 1;
    if (ended && this.state.repeat === "one") index = this.state.index;
    else if (this.state.shuffle && this.state.queue.length > 1)
      index =
        (this.state.index +
          1 +
          Math.floor(Math.random() * (this.state.queue.length - 1))) %
        this.state.queue.length;
    else if (index >= this.state.queue.length && this.state.repeat === "all")
      index = 0;
    await this.open(index);
  }
  remove(index: number) {
    log.info("queue.remove", { index });
    const queue = this.state.queue.filter((_, i) => i !== index);
    if (index === this.state.index) {
      this.update({ queue });
      void this.open(Math.min(index, queue.length - 1));
    } else
      this.update({
        queue,
        index:
          index < this.state.index ? this.state.index - 1 : this.state.index,
      });
  }
  clear() {
    this.selection++;
    log.info("queue.clear", { count: this.state.queue.length });
    this.generation++;
    this.media.pause();
    this.activeSource = false;
    this.media.removeAttribute("src");
    this.media.load();
    this.update({
      current: null,
      queue: [],
      index: -1,
      playing: false,
      loading: false,
      position: 0,
      duration: 0,
    });
  }
}
