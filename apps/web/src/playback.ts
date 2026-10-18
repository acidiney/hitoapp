import type {
  PlaybackQueueInput,
  PlaybackOccurrence,
  PlaybackStart,
  PlaybackView,
  PreparedSelection,
  Player,
  QueueView,
  Track,
} from "./player/model";
import type { AudiobookChapterAnchor } from "./types";
export type { Track } from "./player/model";
import { log } from "./log";
import { createUuid } from "./uuid";
import {
  PlaybackSources,
  type Source,
  type SourceResolver,
} from "./playbackSources";
import { BrowserTraversal } from "./player/BrowserTraversal";
import { BrowserQueue } from "./player/BrowserQueue";
import { BrowserObservation } from "./player/BrowserObservation";
import {
  BrowserPreparation,
  type BrowserMediaOwner,
} from "./player/BrowserPreparation";
export type { Source } from "./playbackSources";
import {
  prepareAudiobookSelection,
  type AudiobookPlayback,
  type AudiobookSelection,
} from "./player/AudiobookContinuation";

export interface MediaPort extends EventTarget {
  src: string;
  currentTime: number;
  duration: number;
  playbackRate: number;
  defaultPlaybackRate: number;
  volume: number;
  paused: boolean;
  ended?: boolean;
  readyState?: number;
  seeking?: boolean;
  currentSrc?: string;
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
  private queue = new BrowserQueue();
  private traversal = new BrowserTraversal();
  private observation: BrowserObservation;
  private openingEntry: PlaybackOccurrence | null = null;
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
  private pendingSeek: number | null = null;
  private activeSource: Source | null = null;
  private lookahead: BrowserPreparation | null = null;
  private openingPreparation: BrowserPreparation | null = null;
  private lookaheadRevision = 0;
  private stopListening: () => void = () => {};
  private stoppingForProgress = false;
  private continuation: AbortController | null = null;

  constructor(
    private media: MediaPort,
    private resolve: SourceResolver,
    private onProgress?: (
      track: Track,
      position: number,
      speed: number,
      chapter?: AudiobookChapterAnchor,
    ) => boolean | void,
    private mediaOwner?: BrowserMediaOwner,
    private prepareAudiobook?: (
      book: string,
      signal: AbortSignal,
    ) => Promise<AudiobookPlayback>,
  ) {
    this.observation = new BrowserObservation((sample) => {
      const result = sample.chapter
        ? this.onProgress?.(
            sample.recording,
            sample.position,
            sample.speed,
            sample.chapter,
          )
        : this.onProgress?.(sample.recording, sample.position, sample.speed);
      return result !== false;
    });
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
      this.observation.metadata();
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
      if (!this.observation.observe()) this.progressBlocked();
    });
    listen("play", () => {
      log.info("playback.started", { recording_id: this.state.current?.id });
      this.update({ playing: true, loading: false });
      this.prepareNext();
    });
    listen("playing", () => {
      if (!this.observation.observe()) this.progressBlocked();
    });
    listen("seeked", () => {
      this.observation.seeked();
      if (!this.observation.observe()) this.progressBlocked();
    });
    listen("pause", () => {
      log.debug("playback.paused", { recording_id: this.state.current?.id });
      if (!this.stoppingForProgress) this.persist();
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
      this.queue.identities(),
      this.state.index,
      this.state.repeat,
    );
    const occurrence = index === null ? undefined : this.queue.at(index);
    const entry = occurrence?.id;
    const recording = occurrence?.recording;
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
    if (this.observation.persist()) return true;
    this.progressBlocked();
    return false;
  }
  private progressBlocked() {
    if (this.stoppingForProgress) return;
    this.stoppingForProgress = true;
    try {
      if (!this.media.paused) this.media.pause();
      this.progressFailed(
        "Listening progress is waiting to be saved. Playback paused.",
      );
    } finally {
      this.stoppingForProgress = false;
    }
  }
  progressFailed(message: string) {
    this.update({ error: message });
  }
  retryProgress() {
    if (
      this.observation.retry() &&
      this.state.error.startsWith("Listening progress")
    )
      this.update({ error: "" });
  }

  snapshot = () => this.state;
  queueSnapshot = () => this.queueView;
  async queuePage(offset: number, limit: number, signal?: AbortSignal) {
    signal?.throwIfAborted();
    if (
      !Number.isInteger(offset) ||
      offset < 0 ||
      offset > this.queue.length ||
      !Number.isInteger(limit) ||
      limit < 1 ||
      limit > 1000
    )
      throw new Error("The queue page is invalid.");
    return {
      version: this.queueView.version,
      total: this.queue.length,
      offset,
      entries: this.queue.page(offset, limit),
    };
  }
  async selectEntry(entry: string) {
    this.cancelAudiobook();
    const index = this.queue.indexOf(entry);
    if (index < 0) return;
    this.traversal.reset(this.queue.identities(), entry);
    this.update({ error: "" });
    await this.open(index);
  }
  removeEntry(entry: string) {
    this.cancelAudiobook();
    const index = this.queue.indexOf(entry);
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
      const current = this.queue.at(this.state.index)?.id;
      this.queueView = {
        version: this.queueView.version + 1,
        total: this.queue.length,
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
    this.queue.updateRecording(update);
    this.update({
      queue: this.queue.recordings(),
      current: this.state.current ? update(this.state.current) : null,
    });
  }

  async playQueue(
    queue: PlaybackQueueInput[],
    index = 0,
    start?: PlaybackStart,
  ) {
    this.cancelAudiobook();
    if (!this.persist())
      throw new Error(
        "Listening progress is waiting to be saved. Playback paused.",
      );
    const position = start?.position ?? 0;
    this.cancelLookahead();
    this.queue.replace(queue);
    this.traversal.reset(this.queue.identities(), this.queue.at(index)?.id);
    log.info("queue.play", {
      count: queue.length,
      index,
      position_ms: Math.floor(position * 1000),
    });
    this.update({ queue: this.queue.recordings(), index, error: "" });
    await this.open(index, position);
  }

  async continueAudiobook(book: string, signal: AbortSignal): Promise<boolean> {
    this.cancelAudiobook();
    const controller = new AbortController();
    this.continuation = controller;
    const combined = AbortSignal.any([signal, controller.signal]);
    const generation = this.generation;
    const selection = ++this.selection;
    const queue = this.queueView;
    const started = performance.now();
    const requestId = createUuid();
    let timedOut = false;
    const deadline = setTimeout(() => {
      timedOut = true;
      controller.abort();
    }, 10_000);
    let cancel!: () => void;
    const cancelled = new Promise<never>((_, reject) => {
      cancel = () => reject(new DOMException("Cancelled", "AbortError"));
      combined.addEventListener("abort", cancel, { once: true });
    });
    const check = () => {
      combined.throwIfAborted();
      if (
        this.continuation !== controller ||
        generation !== this.generation ||
        selection !== this.selection ||
        queue !== this.queueView
      )
        throw new DOMException("Superseded", "AbortError");
    };
    let committed = false;
    log.debug("playback.audiobook_continue_started", { request_id: requestId });
    try {
      check();
      if (!this.prepareAudiobook)
        throw new Error("Audiobook continuation requires a server connection.");
      if (!this.observation.persist())
        throw new Error("Listening progress is waiting to be saved.");
      const fresh = await Promise.race([
        this.prepareAudiobook(book, combined),
        cancelled,
      ]);
      check();
      const target = prepareAudiobookSelection(fresh, book);
      if (!this.observation.persist())
        throw new Error("Listening progress is waiting to be saved.");
      check();
      clearTimeout(deadline);
      this.continuation = null;
      committed = true;
      await this.open(target.index, target.position, target);
      return true;
    } catch (error) {
      if (timedOut && this.continuation === controller && !signal.aborted) {
        this.update({
          error: "Continuing this audiobook timed out. Try again.",
        });
        return false;
      }
      if (
        combined.aborted ||
        (error instanceof DOMException && error.name === "AbortError")
      )
        return false;
      this.update({
        error:
          "Hito could not continue this audiobook. Check its saved chapter and playback speed, then try again.",
      });
      return false;
    } finally {
      clearTimeout(deadline);
      combined.removeEventListener("abort", cancel);
      if (this.continuation === controller) this.continuation = null;
      log.debug("playback.audiobook_continue_finished", {
        request_id: requestId,
        reason: committed
          ? "committed"
          : timedOut
            ? "timeout"
            : combined.aborted
              ? "cancelled"
              : "rejected",
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  private cancelAudiobook() {
    this.continuation?.abort();
    this.continuation = null;
  }

  prepareSelection(): PreparedSelection {
    this.cancelAudiobook();
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

  private async open(
    index: number,
    position = 0,
    continuation?: AudiobookSelection,
  ) {
    if (!continuation) {
      this.cancelAudiobook();
      if (!this.persist()) return;
    }
    this.selection++;
    this.observation.retire();
    if (continuation) {
      this.media.pause();
      this.queue.replace(continuation.queue);
      this.traversal.reset(this.queue.identities(), this.queue.at(index)?.id);
    }
    const occurrence = this.queue.at(index);
    this.openingEntry = occurrence ?? null;
    const current = occurrence?.recording;
    const candidate = this.lookahead;
    const prepared =
      candidate &&
      candidate.target.entry === occurrence?.id &&
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
    if (!continuation) this.media.pause();
    if (continuation) {
      this.media.defaultPlaybackRate = continuation.speed;
      this.media.playbackRate = continuation.speed;
    }
    this.activeSource = null;
    this.media.removeAttribute("src");
    this.media.load();
    this.pendingSeek = position;
    this.update({
      ...(continuation
        ? {
            queue: this.queue.recordings(),
            speed: continuation.speed,
            error: "",
          }
        : {}),
      current: current ?? null,
      index,
      position,
      loading: !!current,
      playing: false,
      duration: (current?.duration_ms ?? 0) / 1000,
    });
    if (!current || !occurrence) {
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
            this.observeSource(occurrence, buffered.source);
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
    const entry = this.openingEntry;
    if (!entry) return;
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
    this.observeSource(entry, source);
    await this.playSource(generation, attempt);
  }

  private observeSource(entry: PlaybackOccurrence, source: Source) {
    const media = this.media;
    const generation = this.generation;
    const activeSource = this.activeSource;
    this.observation.begin(
      entry,
      media,
      `/api/v1/sources/${encodeURIComponent(source.id)}/audio`,
      () =>
        this.media === media &&
        this.generation === generation &&
        this.activeSource === activeSource,
    );
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
    if (!this.activeSource || !this.persist()) return;
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
      this.queue.identities(),
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
    this.cancelAudiobook();
    this.selection++;
    await this.setPlaying(this.media.paused);
  }
  async setPlaying(playing: boolean) {
    this.cancelAudiobook();
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
    this.cancelAudiobook();
    const position = Math.max(
      0,
      Math.min(seconds, this.state.duration || seconds),
    );
    if (this.state.loading && !this.activeSource) this.pendingSeek = position;
    else {
      try {
        const alreadyAtPosition =
          !this.media.seeking &&
          Math.abs(this.media.currentTime - position) < 0.001;
        this.observation.seeking(position);
        this.media.currentTime = position;
        this.pendingSeek = null;
        if (alreadyAtPosition) this.observation.seeked();
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
    this.cancelAudiobook();
    log.info("playback.speed", { speed: value });
    this.media.defaultPlaybackRate = value;
    this.media.playbackRate = value;
    this.update({ speed: value });
  }
  volume(value: number) {
    this.cancelAudiobook();
    log.debug("playback.volume", { volume: value });
    this.media.volume = value;
    this.update({ volume: value });
  }
  repeat() {
    this.cancelAudiobook();
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
    this.cancelAudiobook();
    log.info("queue.shuffle", { enabled: !this.state.shuffle });
    this.cancelLookahead();
    this.update({
      shuffle: this.traversal.toggle(
        this.queue.identities(),
        this.queue.at(this.state.index)?.id,
      ),
    });
    this.prepareNext();
  }
  async previous() {
    this.cancelAudiobook();
    this.selection++;
    this.traversal.resetFailures();
    log.info("queue.previous", { index: this.state.index });
    if (this.media.currentTime > 3) this.seek(0);
    else await this.open(Math.max(0, this.state.index - 1));
  }
  async next(ended = false) {
    this.cancelAudiobook();
    log.info("queue.next", {
      index: this.state.index,
      reason: ended ? "ended" : "requested",
    });
    const index = this.traversal.next(
      this.queue.identities(),
      this.state.index,
      this.state.repeat,
      ended ? "completed" : "requested",
    );
    await this.open(index ?? this.queue.length);
  }
  remove(index: number) {
    this.cancelAudiobook();
    if (index === this.state.index && !this.persist()) return;
    log.info("queue.remove", { index });
    this.cancelLookahead();
    const next =
      index === this.state.index
        ? this.traversal.next(
            this.queue.identities(),
            index,
            this.state.repeat,
            "requested",
          )
        : null;
    const entry = this.queue.at(index)?.id;
    if (!entry) return;
    this.queue.remove(index);
    const queue = this.queue.recordings();
    this.traversal.remove(entry, this.queue.identities());
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
    this.cancelAudiobook();
    if (!this.persist()) return;
    this.observation.retire();
    this.openingEntry = null;
    this.cancelLookahead();
    this.openingPreparation?.close();
    this.openingPreparation = null;
    this.queue.clear();
    this.traversal.reset(this.queue.identities());
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
  retire() {
    this.observation.retire();
    this.clear();
  }
}
