import type { PlaybackView, Player, QueueView, Track } from "./player/model";
export type { Track } from "./player/model";
import { log } from "./log";
import { PlaybackSources, type SourceResolver } from "./playbackSources";
import { BrowserTraversal } from "./player/BrowserTraversal";
export type { Source } from "./playbackSources";

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
export type PlaybackState = PlaybackView & {
  queue: Track[];
  index: number;
  position: number;
  duration: number;
};

export class Playback implements Player {
  readonly mode = "browser";
  readonly supports = {
    previous: true,
    shuffle: true,
    repeat: true,
    speed: true,
  };
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
    volume: 1,
    connection: "ready",
  };
  private entryIds: string[] = [];
  private traversal = new BrowserTraversal();
  private queueView: QueueView = {
    version: 0,
    total: 0,
    currentIndex: null,
    currentEntry: null,
  };
  private listeners = new Set<() => void>();
  private position = { position: 0, duration: 0 };
  private positionListeners = new Set<() => void>();
  private sources: PlaybackSources | null = null;
  private generation = 0;
  private selection = 0;
  private sourceAttempt = 0;
  private lastSavedPosition = 0;
  private pendingSeek: number | null = null;
  private activeSource = false;

  constructor(
    private media: MediaPort,
    private resolve: SourceResolver,
    private onProgress?: (
      track: Track,
      position: number,
      speed: number,
    ) => void,
  ) {
    media.addEventListener("loadedmetadata", () => {
      log.debug("playback.metadata", { recording_id: this.state.current?.id });
      this.update({
        duration: Number.isFinite(media.duration) ? media.duration : 0,
      });
      if (this.pendingSeek !== null) {
        const position = this.pendingSeek;
        this.pendingSeek = null;
        this.seek(position);
      }
    });
    media.addEventListener("timeupdate", () => {
      if (this.activeSource && !media.paused && media.currentTime > 0)
        this.traversal.resetFailures();
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
  queueSnapshot = () => this.queueView;
  async queuePage(offset: number, limit: number, signal?: AbortSignal) {
    signal?.throwIfAborted();
    if (
      !Number.isInteger(offset) ||
      offset < 0 ||
      offset > this.state.queue.length ||
      !Number.isInteger(limit) ||
      limit < 1 ||
      limit > 1000
    )
      throw new Error("The queue page is invalid.");
    return {
      version: this.queueView.version,
      total: this.state.queue.length,
      offset,
      entries: this.state.queue
        .slice(offset, offset + limit)
        .map((recording, index) => ({
          id: this.entryIds[offset + index],
          recording,
        })),
    };
  }
  async selectEntry(entry: string) {
    const index = this.entryIds.indexOf(entry);
    if (index < 0) return;
    this.traversal.resetFailures();
    this.update({ error: "" });
    await this.open(index);
  }
  removeEntry(entry: string) {
    const index = this.entryIds.indexOf(entry);
    if (index >= 0) this.remove(index);
  }
  positionSnapshot = () => this.position;
  subscribePosition = (listener: () => void) => {
    this.positionListeners.add(listener);
    return () => {
      this.positionListeners.delete(listener);
    };
  };
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  private update(patch: Partial<PlaybackState>) {
    const { position, duration, ...structure } = patch;
    const moved =
      (position !== undefined && position !== this.position.position) ||
      (duration !== undefined && duration !== this.position.duration);
    this.state = { ...this.state, ...patch };
    if (patch.queue !== undefined || patch.index !== undefined) {
      const current = this.entryIds[this.state.index];
      this.queueView = {
        version: this.queueView.version + 1,
        total: this.state.queue.length,
        currentIndex: current ? this.state.index : null,
        currentEntry: current ?? null,
      };
    }
    if (moved) {
      this.position = {
        position: this.state.position,
        duration: this.state.duration,
      };
      this.positionListeners.forEach((listener) => listener());
    }
    if (Object.keys(structure).length)
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
    this.entryIds = queue.map(() => crypto.randomUUID());
    this.traversal.reset(this.entryIds, this.entryIds[index]);
    log.info("queue.play", {
      count: queue.length,
      index,
      position_ms: Math.floor(position * 1000),
    });
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
    this.sources?.close();
    this.sources = current
      ? new PlaybackSources(current.id, this.resolve)
      : null;
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
      await this.useNextSource(position);
    } catch {
      log.warn("playback.resolution_failed", { recording_id: current.id });
      if (generation === this.generation) await this.skipFailed();
    }
  }

  private async useNextSource(position: number) {
    const generation = this.generation;
    const attempt = ++this.sourceAttempt;
    const sources = this.sources;
    const source = await sources?.next();
    if (
      generation !== this.generation ||
      attempt !== this.sourceAttempt ||
      sources !== this.sources
    )
      return;
    if (!source) {
      await this.skipFailed();
      return;
    }
    this.pendingSeek ??= position;
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
    if (!this.activeSource) return;
    const generation = this.generation;
    const sources = this.sources;
    this.activeSource = false;
    const position = this.pendingSeek ?? this.media.currentTime;
    this.pendingSeek = position;
    this.media.pause();
    this.update({ playing: false, loading: true });
    log.warn("playback.recovering", {
      recording_id: this.state.current?.id,
    });
    try {
      await this.useNextSource(position);
    } catch {
      if (generation === this.generation && sources === this.sources)
        await this.skipFailed();
    }
  }

  private async skipFailed() {
    log.warn("playback.unavailable", {
      recording_id: this.state.current?.id,
      index: this.state.index,
    });
    this.update({
      error: `Audio is unavailable for ${this.state.current?.title ?? "this track"}.`,
      loading: false,
    });
    const next = this.traversal.next(
      this.entryIds,
      this.entryIds[this.state.index],
      this.state.repeat,
      "failed",
    );
    if (next) await this.open(this.entryIds.indexOf(next));
    else {
      this.media.pause();
      this.update({ playing: false, loading: false });
    }
  }

  async toggle() {
    this.selection++;
    if (this.state.loading) return;
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
    if (!Number.isFinite(seconds)) return;
    const position = Math.max(
      0,
      Math.min(seconds, this.state.duration || seconds),
    );
    if (this.state.loading && !this.activeSource) this.pendingSeek = position;
    else {
      try {
        this.media.currentTime = position;
        this.pendingSeek = null;
      } catch {
        this.pendingSeek = position;
        void this.recover();
      }
    }
    this.update({ position });
    log.debug("playback.seek", {
      recording_id: this.state.current?.id,
      position_ms: Math.floor(position * 1000),
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
    this.update({ volume: value });
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
    this.update({
      shuffle: this.traversal.toggle(
        this.entryIds,
        this.entryIds[this.state.index],
      ),
    });
  }
  async previous() {
    this.selection++;
    this.traversal.resetFailures();
    log.info("queue.previous", { index: this.state.index });
    if (this.media.currentTime > 3) this.seek(0);
    else await this.open(Math.max(0, this.state.index - 1));
  }
  async next(ended = false) {
    log.info("queue.next", {
      index: this.state.index,
      reason: ended ? "ended" : "requested",
    });
    const entry = this.traversal.next(
      this.entryIds,
      this.entryIds[this.state.index],
      this.state.repeat,
      ended ? "completed" : "requested",
    );
    await this.open(
      entry ? this.entryIds.indexOf(entry) : this.state.queue.length,
    );
  }
  remove(index: number) {
    log.info("queue.remove", { index });
    const queue = this.state.queue.filter((_, i) => i !== index);
    this.traversal.remove(this.entryIds[index]);
    this.entryIds.splice(index, 1);
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
    this.entryIds = [];
    this.traversal.reset(this.entryIds);
    this.selection++;
    log.info("queue.clear", { count: this.state.queue.length });
    this.generation++;
    this.sources?.close();
    this.sources = null;
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
