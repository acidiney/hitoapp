import { createUuid } from "./uuid";
import type {
  PlaybackStart,
  PlaybackView,
  PreparedSelection,
  Player,
  QueueView,
  Track,
} from "./player/model";
export type { Track } from "./player/model";
import { log } from "./log";
import {
  PlaybackSources,
  type Source,
  type SourceResolver,
} from "./playbackSources";
import { BrowserTraversal } from "./player/BrowserTraversal";
import {
  BrowserPreparation,
  type BrowserMediaOwner,
} from "./player/BrowserPreparation";
export type { Source } from "./playbackSources";

export interface MediaPort extends EventTarget {
  src: string;
  currentTime: number;
  duration: number;
  playbackRate: number;
  defaultPlaybackRate: number;
  volume: number;
  paused: boolean;
  ended?: boolean;
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
  private activeSource: Source | null = null;
  private lookahead: BrowserPreparation | null = null;
  private openingPreparation: BrowserPreparation | null = null;
  private lookaheadRevision = 0;
  private stopListening: () => void = () => {};

  constructor(
    private media: MediaPort,
    private resolve: SourceResolver,
    private onProgress?: (
      track: Track,
      position: number,
      speed: number,
    ) => void,
    private mediaOwner?: BrowserMediaOwner,
  ) {
    this.listenMedia();
  }

  private listenMedia() {
    const media = this.media;
    const listeners: [string, EventListener][] = [];
    const listen = (event: string, action: () => void) => {
      const listener = () => {
        if (this.media === media) action();
      };
      media.addEventListener(event, listener);
      listeners.push([event, listener]);
    };
    this.stopListening = () => {
      for (const [event, listener] of listeners)
        media.removeEventListener(event, listener);
    };
    listen("loadedmetadata", () => {
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
    listen("timeupdate", () => {
      if (this.activeSource && !media.paused && media.currentTime > 0) {
        this.traversal.resetFailures();
        this.prepareNext();
      }
      this.update({ position: media.currentTime });
      if (Math.abs(media.currentTime - this.lastSavedPosition) >= 10)
        this.persist();
    });
    listen("play", () => {
      log.info("playback.started", { recording_id: this.state.current?.id });
      this.update({ playing: true, loading: false });
      this.prepareNext();
    });
    listen("pause", () => {
      log.debug("playback.paused", { recording_id: this.state.current?.id });
      this.persist();
      this.update({ playing: false });
      if (!media.ended) this.cancelLookahead();
    });
    listen("error", () => {
      log.warn("playback.media_failed", {
        recording_id: this.state.current?.id,
      });
      if (this.activeSource) void this.recover();
    });
    listen("ended", () => {
      log.info("playback.ended", { recording_id: this.state.current?.id });
      void this.next(true);
    });
  }

  private cancelLookahead() {
    this.lookaheadRevision++;
    this.lookahead?.close();
    this.lookahead = null;
  }

  private prepareNext() {
    if (!this.mediaOwner || !this.state.playing || !this.activeSource) return;
    const index = this.traversal.completed(
      this.entryIds,
      this.state.index,
      this.state.repeat,
    );
    const entry = index === null ? undefined : this.entryIds[index];
    const recording = index === null ? undefined : this.state.queue[index];
    if (!entry || !recording) {
      this.cancelLookahead();
      return;
    }
    const target = this.lookahead?.target;
    if (
      target?.entry === entry &&
      target.recording === recording.id &&
      target.generation === this.generation &&
      target.revision === this.lookaheadRevision
    )
      return;
    const seed =
      recording.id === this.state.current?.id && this.activeSource.ready
        ? { ...this.activeSource }
        : undefined;
    this.cancelLookahead();
    this.lookahead = new BrowserPreparation(
      {
        generation: this.generation,
        revision: this.lookaheadRevision,
        entry,
        recording: recording.id,
      },
      this.resolve,
      this.mediaOwner,
      seed,
    );
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
    this.traversal.reset(this.entryIds, entry);
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

  async playQueue(queue: Track[], index = 0, start?: PlaybackStart) {
    const position = start?.position ?? 0;
    this.cancelLookahead();
    this.entryIds = queue.map(() => createUuid());
    this.traversal.reset(this.entryIds, this.entryIds[index]);
    log.info("queue.play", {
      count: queue.length,
      index,
      position_ms: Math.floor(position * 1000),
    });
    this.update({ queue: [...queue], index, error: "" });
    await this.open(index, position);
  }

  prepareSelection(): PreparedSelection {
    const selection = ++this.selection;
    log.debug("playback.selection_prepared");
    return async (target) => {
      if (selection !== this.selection) {
        log.debug("playback.selection_superseded");
        return false;
      }
      if ("queue" in target)
        await this.playQueue(target.queue, target.index, target.start);
      else await this.playQueue([target]);
      return true;
    };
  }

  private async open(index: number, position = 0) {
    this.selection++;
    this.persist();
    this.lastSavedPosition = 0;
    const current = this.state.queue[index];
    const candidate = this.lookahead;
    const prepared =
      candidate &&
      candidate.target.entry === this.entryIds[index] &&
      candidate.target.recording === current?.id &&
      candidate.target.generation === this.generation &&
      candidate.target.revision === this.lookaheadRevision
        ? candidate
        : null;
    if (prepared) this.lookahead = null;
    this.cancelLookahead();
    this.openingPreparation?.close();
    this.openingPreparation = prepared;
    const generation = ++this.generation;
    log.info("playback.resolving", { recording_id: current?.id, index });
    this.sources?.close();
    this.sources =
      prepared?.sources ??
      (current ? new PlaybackSources(current.id, this.resolve) : null);
    this.media.pause();
    this.activeSource = null;
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
      if (prepared) {
        const buffered = await prepared.read();
        if (
          generation !== this.generation ||
          this.openingPreparation !== prepared
        ) {
          prepared.close();
          return;
        }
        this.openingPreparation = null;
        if (buffered && prepared.transfer()) {
          if (buffered.media && buffered.source) {
            const previous = this.media;
            this.stopListening();
            this.media = buffered.media;
            this.listenMedia();
            this.mediaOwner?.activate(this.media);
            this.mediaOwner?.release(previous);
            this.media.volume = this.state.volume;
            this.media.defaultPlaybackRate = this.state.speed;
            this.media.playbackRate = this.state.speed;
            this.activeSource = { ...buffered.source };
            this.update({
              duration: Number.isFinite(this.media.duration)
                ? this.media.duration
                : (current.duration_ms ?? 0) / 1000,
            });
            const seek = this.pendingSeek ?? position;
            if (seek > 0) {
              this.seek(seek);
              if (!this.activeSource) return;
            } else this.pendingSeek = null;
            log.debug("playback.next_preparation_adopted", {
              recording_id: current.id,
              operation_id: prepared.id,
              source_id: buffered.source.id,
            });
            await this.playSource(generation, ++this.sourceAttempt);
            return;
          }
        } else this.sources = new PlaybackSources(current.id, this.resolve);
      }
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
    this.activeSource = { ...source };
    log.info("playback.source_changed", {
      recording_id: this.state.current?.id,
      source_id: source.id,
      position_ms: Math.floor(position * 1000),
    });
    this.media.src = `/api/v1/sources/${encodeURIComponent(source.id)}/audio`;
    this.media.load();
    await this.playSource(generation, attempt);
  }

  private async playSource(generation: number, attempt: number) {
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
    this.activeSource = null;
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
      this.state.index,
      this.state.repeat,
      "failed",
    );
    if (next !== null) await this.open(next);
    else {
      this.media.pause();
      this.update({ playing: false, loading: false });
    }
  }

  async toggle() {
    this.selection++;
    await this.setPlaying(this.media.paused);
  }
  async setPlaying(playing: boolean) {
    if (this.state.loading) return;
    log.info("playback.playing_set", {
      recording_id: this.state.current?.id,
      enabled: playing,
    });
    if (playing === !this.media.paused) return;
    const generation = this.generation;
    const attempt = ++this.sourceAttempt;
    if (playing) {
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
    this.cancelLookahead();
    this.update({
      repeat:
        this.state.repeat === "off"
          ? "all"
          : this.state.repeat === "all"
            ? "one"
            : "off",
    });
    this.prepareNext();
  }
  shuffle() {
    log.info("queue.shuffle", { enabled: !this.state.shuffle });
    this.cancelLookahead();
    this.update({
      shuffle: this.traversal.toggle(
        this.entryIds,
        this.entryIds[this.state.index],
      ),
    });
    this.prepareNext();
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
    const index = this.traversal.next(
      this.entryIds,
      this.state.index,
      this.state.repeat,
      ended ? "completed" : "requested",
    );
    await this.open(index ?? this.state.queue.length);
  }
  remove(index: number) {
    log.info("queue.remove", { index });
    this.cancelLookahead();
    const next =
      index === this.state.index
        ? this.traversal.next(
            this.entryIds,
            index,
            this.state.repeat,
            "requested",
          )
        : null;
    const queue = this.state.queue.filter((_, i) => i !== index);
    const entry = this.entryIds[index];
    this.entryIds.splice(index, 1);
    this.traversal.remove(entry, this.entryIds);
    if (index === this.state.index) {
      this.update({ queue });
      const successor =
        next === null || next === index
          ? queue.length
          : next > index
            ? next - 1
            : next;
      void this.open(successor);
    } else
      this.update({
        queue,
        index:
          index < this.state.index ? this.state.index - 1 : this.state.index,
      });
    this.prepareNext();
  }
  clear() {
    this.cancelLookahead();
    this.openingPreparation?.close();
    this.openingPreparation = null;
    this.entryIds = [];
    this.traversal.reset(this.entryIds);
    this.selection++;
    log.info("queue.clear", { count: this.state.queue.length });
    this.generation++;
    this.sources?.close();
    this.sources = null;
    this.media.pause();
    this.activeSource = null;
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
