import type { Schema } from "../../shared/schema";
import {
  MAX_QUEUE_ENTRIES,
  MEDIA_ERR_SRC_NOT_SUPPORTED,
  UNSUPPORTED_FORMAT_MESSAGE,
  type MediaPort,
  type PlaybackQueueInput,
  type QueueSupply,
  type PlaybackOccurrence,
  type PlaybackStart,
  type PlaybackState,
  type PreparedSelection,
  type Player,
  type Track,
} from "./model";

export type { MediaPort, Track } from "./model";
import { log } from "../../shared/log";
import {
  PlaybackSources,
  type Source,
  type SourcePreparer,
  type SourceResolver,
} from "./api/playbackSources";
import { sourceWasDelivered } from "./browser/BrowserFormatProbe";
import { BrowserTraversal } from "./browser/BrowserTraversal";
import { BrowserQueue } from "./browser/BrowserQueue";
import { BrowserObservation } from "./browser/BrowserObservation";
import {
  BrowserProgressGuard,
  PROGRESS_WAITING_MESSAGE,
} from "./browser/BrowserProgressGuard";
import {
  acceptedCount,
  indexAfterMove,
  indexAfterRemove,
  isValidPage,
  nextRepeat,
  planMove,
  previousIndex,
  successorAfterRemove,
} from "./browser/QueueModel";
import { BrowserCommunityObservation } from "./browser/BrowserCommunityObservation";

import {
  BrowserPreparation,
  type BrowserMediaOwner,
} from "./browser/BrowserPreparation";
import { BrowserLookahead } from "./browser/BrowserLookahead";
import { BrowserEndGuard, reconcileDuration } from "./browser/BrowserDuration";
import { listenToMedia } from "./browser/BrowserMediaEvents";
import { applyBrowserSpeed } from "./browser/BrowserSpeed";
import {
  BrowserAudiobookRun,
  type AudiobookPreparer,
} from "./browser/BrowserAudiobookRun";
import { PlaybackStore } from "./browser/PlaybackStore";
import { QueueFeed } from "./QueueFeed";
import type { AudiobookSelection } from "./browser/AudiobookContinuation";
export type { Source } from "./api/playbackSources";

export class Playback implements Player {
  readonly mode = "browser";
  readonly supports = {
    previous: true,
    shuffle: true,
    repeat: true,
    speed: true,
  };
  private store = new PlaybackStore();
  private queue = new BrowserQueue();
  private traversal = new BrowserTraversal();
  private observation: BrowserObservation;
  private community: BrowserCommunityObservation | null;
  private openingEntry: PlaybackOccurrence | null = null;
  private sources: PlaybackSources | null = null;
  private generation = 0;
  private selection = 0;
  private sourceAttempt = 0;
  private pendingSeek: number | null = null;
  private activeSource: Source | null = null;
  private durationChanged: (() => void) | null = null;
  private serverBound: number | null = null;
  private entryEnded = false;
  private formatProbe: Promise<boolean> = Promise.resolve(false);
  private endGuard = new BrowserEndGuard(() => this.endAtKnownDuration());
  private lookahead: BrowserLookahead | null;
  private stopListening: () => void = () => {};
  private progress: BrowserProgressGuard;
  private audiobook: BrowserAudiobookRun;
  private feed = new QueueFeed(
    () => this.store.setPending(this.feed.pending),
    (error) => this.update({ error }),
  );

  constructor(
    private media: MediaPort,
    private resolve: SourceResolver,
    private onProgress?: (
      track: Track,
      position: number,
      speed: number,
      chapter?: Schema<"AudiobookChapterAnchor">,
    ) => boolean | void,
    private mediaOwner?: BrowserMediaOwner,
    prepareAudiobook?: AudiobookPreparer,
    onSuccess?: (source: string, claim: Schema<"SuccessfulPlayback">) => void,
    sourcePreparer?: SourcePreparer,
    private fallbackAllowed?: () => Promise<boolean>,
  ) {
    this.community = onSuccess
      ? new BrowserCommunityObservation(onSuccess)
      : null;
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
    this.progress = new BrowserProgressGuard(
      this.observation,
      {
        isPaused: () => this.media.paused,
        pause: () => this.media.pause(),
      },
      (message) => this.update({ error: message }),
    );
    this.lookahead = mediaOwner
      ? new BrowserLookahead(
          resolve,
          mediaOwner,
          sourcePreparer,
          fallbackAllowed,
        )
      : null;
    this.audiobook = new BrowserAudiobookRun(
      {
        generation: () => this.generation,
        selection: () => this.selection,
        claimSelection: () => ++this.selection,
        queueView: () => this.store.queueView,
        persist: () => this.observation.persist(),
        open: (target) => this.open(target.index, target.position, target),
        fail: (error) => this.update({ error }),
      },
      prepareAudiobook,
    );
    this.listenMedia();
  }

  private get state() {
    return this.store.state;
  }

  private applySpeed(chosen: number) {
    const { effective, range } = applyBrowserSpeed(this.media, chosen);
    const current = this.state.speedRange;
    if (
      this.state.effectiveSpeed !== effective ||
      current?.min !== range.min ||
      current?.max !== range.max
    ) {
      if (effective !== chosen)
        log.info("playback.speed_unsupported", { speed: chosen });
      this.update({ effectiveSpeed: effective, speedRange: range });
    }
  }

  private listenMedia() {
    const media = this.media;
    this.stopListening = listenToMedia(media, () => this.media === media, {
      loadedMetadata: () => {
        this.observation.metadata();
        this.entryEnded = false;
        log.debug("playback.metadata", {
          recording_id: this.state.current?.id,
        });
        this.update({ duration: this.reconcile(media).seconds });
        this.applySpeed(this.state.speed);
        if (this.pendingSeek !== null) {
          const position = this.pendingSeek;
          this.pendingSeek = null;
          this.seek(position);
        }
      },
      durationChange: () => this.durationChanged?.(),
      timeUpdate: () => {
        this.community?.observe();
        if (this.activeSource && !media.paused && media.currentTime > 0) {
          this.traversal.resetFailures();
          this.prepareNext();
        }
        this.update({ position: this.pendingSeek || media.currentTime });
        this.progress.observe();
        if (this.activeSource) this.endGuard.observe(media, this.serverBound);
      },
      play: () => {
        log.info("playback.started", { recording_id: this.state.current?.id });
        this.update({ playing: true, loading: false });
        this.prepareNext();
      },
      playing: () => {
        this.community?.observe();
        this.progress.observe();
      },
      seeked: () => {
        this.community?.seeked();
        this.community?.observe();
        this.observation.seeked();
        this.progress.observe();
      },
      pause: () => {
        this.community?.reset();
        this.endGuard.cancel();
        log.debug("playback.paused", { recording_id: this.state.current?.id });
        if (!this.progress.stopping) this.persist();
        this.update({ playing: false });
        if (!media.ended) this.cancelLookahead();
      },
      error: () => {
        this.community?.reset();
        const rejected = media.error?.code === MEDIA_ERR_SRC_NOT_SUPPORTED;
        log.warn("playback.media_failed", {
          recording_id: this.state.current?.id,
          reason: rejected ? "format_rejected" : "media_error",
        });
        if (this.activeSource) {
          if (rejected) this.probeFormat(this.activeSource);
          void this.recover();
        }
      },
      ended: () => {
        if (!this.activeSource || this.entryEnded) return;
        this.entryEnded = true;
        this.endGuard.cancel();
        log.info("playback.ended", { recording_id: this.state.current?.id });
        void this.next(true);
      },
      seeking: () => {
        this.endGuard.cancel();
        this.community?.seeking();
      },
      canPlay: () => this.community?.observe(),
      interrupted: () => this.community?.reset(),
    });
  }

  private reconcile(media: MediaPort) {
    const current = this.state.current;
    const duration = reconcileDuration(media.duration, current?.duration_ms);
    const bound = duration.server ? duration.seconds : null;
    if (bound !== null && bound !== this.serverBound)
      log.debug("playback.duration_server_preferred", {
        recording_id: current?.id,
        media_duration_ms: Math.round(media.duration * 1000),
        server_duration_ms: Math.round(bound * 1000),
      });
    this.serverBound = bound;
    return duration;
  }

  private endAtKnownDuration() {
    if (!this.activeSource || this.entryEnded) return;
    this.entryEnded = true;
    log.info("playback.ended_at_known_duration", {
      recording_id: this.state.current?.id,
    });
    void this.next(true);
  }

  private cancelLookahead() {
    this.lookahead?.cancel();
  }

  private prepareNext() {
    if (!this.lookahead || !this.state.playing || !this.activeSource) return;
    const index = this.traversal.completed(
      this.queue.serials(),
      this.state.index,
      this.state.repeat,
    );
    this.lookahead.prepare(index === null ? undefined : this.queue.at(index), {
      generation: this.generation,
      currentRecording: this.state.current?.id,
      active: this.activeSource,
    });
  }

  private persist() {
    return this.progress.persist();
  }
  progressFailed(message: string) {
    this.update({ error: message });
  }
  retryProgress() {
    if (
      this.progress.retry() &&
      this.state.error.startsWith("Listening progress")
    )
      this.update({ error: "" });
  }

  snapshot = this.store.snapshot;
  queueSnapshot = this.store.queueSnapshot;
  async queuePage(offset: number, limit: number, signal?: AbortSignal) {
    signal?.throwIfAborted();
    if (!isValidPage(offset, limit, this.queue.length))
      throw new Error("The queue page is invalid.");
    return {
      version: this.store.queueView.version,
      total: this.queue.length,
      offset,
      entries: this.queue.page(offset, limit),
    };
  }
  async selectEntry(entry: string) {
    this.cancelAudiobook();
    const index = this.queue.indexOf(entry);
    if (index < 0) return;
    this.traversal.reset(this.queue.serials(), this.queue.serialAt(index));
    this.update({ error: "" });
    await this.open(index);
  }
  removeEntry(entry: string) {
    this.cancelAudiobook();
    const index = this.queue.indexOf(entry);
    if (index >= 0) this.remove(index);
  }
  moveEntry(entry: string, before: string | null) {
    const from = this.queue.indexOf(entry);
    if (from < 0 || before === entry) return;
    const target = before === null ? null : this.queue.indexOf(before);
    if (target === -1) return;
    const to = planMove(this.queue.length, from, target);
    if (to === null) return;
    this.cancelAudiobook();
    const current = this.state.index;
    this.queue.move(from, to);
    log.info("queue.move", { action: "move", index: to });
    this.update({
      queueChanged: true,
      index: indexAfterMove(current, from, to),
    });
    this.prepareNext();
  }
  private appendBatch(batch: PlaybackQueueInput[]) {
    const accepted = batch.slice(
      0,
      acceptedCount(this.queue.length, batch.length, MAX_QUEUE_ENTRIES),
    );
    if (!accepted.length) return 0;
    const first = this.queue.length;
    this.queue.append(accepted);
    this.traversal.append(
      this.queue.serials().slice(first),
      this.queue.serialAt(this.state.index),
    );
    log.debug("queue.appended", { count: accepted.length });
    this.update({ queueChanged: true });
    this.prepareNext();
    return accepted.length;
  }
  positionSnapshot = this.store.positionSnapshot;
  subscribePosition = this.store.subscribePosition;
  subscribe = this.store.subscribe;
  private update(patch: Partial<PlaybackState> & { queueChanged?: true }) {
    this.store.update(patch, this.queue);
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
      queueChanged: true,
      current: this.state.current ? update(this.state.current) : null,
    });
  }

  async playQueue(
    queue: PlaybackQueueInput[],
    index = 0,
    start?: PlaybackStart,
    more?: QueueSupply,
  ) {
    this.cancelAudiobook();
    if (!this.persist()) throw new Error(PROGRESS_WAITING_MESSAGE);
    const position = start?.position ?? 0;
    this.cancelLookahead();
    this.feed.cancel();
    this.queue.replace(queue);
    this.traversal.reset(this.queue.serials(), this.queue.serialAt(index));
    log.info("queue.play", {
      count: queue.length,
      index,
      position_ms: Math.floor(position * 1000),
    });
    this.update({ queueChanged: true, index, error: "" });
    this.feed.start(more, async (batch) => this.appendBatch(batch));
    await this.open(index, position);
  }

  continueAudiobook(book: string, signal: AbortSignal): Promise<boolean> {
    return this.audiobook.run(book, signal);
  }

  private cancelAudiobook() {
    this.audiobook.cancel();
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
        await this.playQueue(
          target.queue,
          target.index,
          target.start,
          target.more,
        );
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
    this.community?.retire();
    this.endGuard.cancel();
    this.serverBound = null;
    if (continuation) {
      this.feed.cancel();
      this.media.pause();
      this.queue.replace(continuation.queue);
      this.traversal.reset(this.queue.serials(), this.queue.serialAt(index));
    }
    const occurrence = this.queue.at(index);
    this.openingEntry = occurrence ?? null;
    const current = occurrence?.recording;
    const prepared = this.lookahead?.claim(occurrence, this.generation) ?? null;
    const generation = ++this.generation;
    log.info("playback.resolving", { recording_id: current?.id, index });
    this.sources?.close();
    this.sources =
      prepared?.sources ??
      (current
        ? new PlaybackSources(
            current.id,
            this.resolve,
            undefined,
            this.fallbackAllowed,
          )
        : null);
    if (!continuation) this.media.pause();
    this.activeSource = null;
    this.formatProbe = Promise.resolve(false);
    this.media.removeAttribute("src");
    this.media.load();
    if (continuation) this.applySpeed(continuation.speed);
    this.pendingSeek = position;
    this.update({
      ...(continuation
        ? {
            queueChanged: true,
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
          !this.lookahead?.isOpening(prepared)
        ) {
          prepared.close();
          return;
        }
        this.lookahead?.settleOpening();
        if (buffered && prepared.transfer()) {
          if (buffered.media && buffered.source) {
            const previous = this.media;
            this.stopListening();
            this.media = buffered.media;
            this.listenMedia();
            this.mediaOwner?.activate(this.media);
            this.mediaOwner?.release(previous);
            this.media.volume = this.state.volume;
            this.applySpeed(this.state.speed);
            this.activeSource = { ...buffered.source };
            this.observeSource(occurrence, buffered.source);
            this.entryEnded = false;
            this.update({
              duration: Number.isFinite(this.media.duration)
                ? this.reconcile(this.media).seconds
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
        } else
          this.sources = new PlaybackSources(
            current.id,
            this.resolve,
            undefined,
            this.fallbackAllowed,
          );
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
    const path = `/api/v1/sources/${encodeURIComponent(source.id)}/audio`;
    const current = () =>
      this.media === media &&
      this.generation === generation &&
      this.activeSource === activeSource;
    const sourceCurrent = () =>
      current() &&
      new URL(media.currentSrc || media.src, "http://hito.invalid").pathname ===
        path;
    this.durationChanged = () => {
      const started = performance.now();
      if (!sourceCurrent()) return;
      const known = Number.isFinite(media.duration) && media.duration >= 0;
      if (!known && (media.readyState ?? 1) < 1) return;
      const duration = this.reconcile(media).seconds;
      if (duration === this.state.duration) return;
      this.update({ duration });
      log.debug("playback.duration_changed", {
        recording_id: entry.recording.id,
        reason: known ? "finite" : "unknown",
        duration_ms: Math.round(performance.now() - started),
      });
    };
    this.observation.begin(entry, media, path, current);
    this.community?.begin(media, source.id, sourceCurrent);
  }

  private async playSource(generation: number, attempt: number) {
    try {
      await this.media.play();
    } catch (error) {
      if (generation !== this.generation || attempt !== this.sourceAttempt)
        return;
      if (error instanceof Error && error.name === "NotAllowedError")
        this.update({ loading: false, error: "Press play to start audio." });
      else {
        if (
          error instanceof Error &&
          error.name === "NotSupportedError" &&
          this.activeSource
        )
          this.probeFormat(this.activeSource);
        await this.recover();
      }
    }
  }

  async recover() {
    if (!this.activeSource || !this.persist()) return;
    const generation = this.generation;
    const sources = this.sources;
    this.activeSource = null;
    const position =
      this.pendingSeek ??
      (this.media.currentTime > 0
        ? this.media.currentTime
        : this.state.position);
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

  private probeFormat(source: Source) {
    this.formatProbe = sourceWasDelivered(
      `/api/v1/sources/${encodeURIComponent(source.id)}/audio`,
    );
  }

  private async skipFailed() {
    const generation = this.generation;
    const formatRejected = await this.formatProbe;
    if (generation !== this.generation) return;
    const providerUnavailable =
      this.sources?.outcome === "provider_unavailable";
    log.warn("playback.unavailable", {
      recording_id: this.state.current?.id,
      index: this.state.index,
      reason: providerUnavailable
        ? "provider_unavailable"
        : formatRejected
          ? "format_unsupported"
          : "exhausted",
    });
    this.update({
      error: providerUnavailable
        ? "An audio service is unavailable. Select the track again to retry."
        : formatRejected
          ? UNSUPPORTED_FORMAT_MESSAGE
          : `Audio is unavailable for ${this.state.current?.title ?? "this track"}.`,
      loading: false,
    });
    const next = this.traversal.next(
      this.queue.serials(),
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
    const awaitingMetadata =
      this.activeSource !== null && (this.media.readyState ?? 1) < 1;
    if ((this.state.loading && !this.activeSource) || awaitingMetadata)
      this.pendingSeek = position;
    else {
      try {
        const alreadyAtPosition =
          !this.media.seeking &&
          Math.abs(this.media.currentTime - position) < 0.001;
        this.observation.seeking(position);
        this.community?.seeking();
        this.media.currentTime = position;
        this.pendingSeek = null;
        if (alreadyAtPosition) {
          this.observation.seeked();
          this.community?.seeked();
        }
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
    this.update({ speed: value });
    this.applySpeed(value);
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
      repeat: nextRepeat(this.state.repeat),
    });
    this.prepareNext();
  }
  shuffle() {
    this.cancelAudiobook();
    log.info("queue.shuffle", { enabled: !this.state.shuffle });
    this.cancelLookahead();
    this.update({
      shuffle: this.traversal.toggle(
        this.queue.serials(),
        this.queue.serialAt(this.state.index),
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
    else await this.open(previousIndex(this.state.index));
  }
  async next(ended = false) {
    this.cancelAudiobook();
    log.info("queue.next", {
      index: this.state.index,
      reason: ended ? "ended" : "requested",
    });
    const index = this.traversal.next(
      this.queue.serials(),
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
            this.queue.serials(),
            index,
            this.state.repeat,
            "requested",
          )
        : null;
    const serial = this.queue.serialAt(index);
    if (serial === undefined) return;
    this.queue.remove(index);
    this.traversal.remove(serial);
    if (index === this.state.index) {
      this.update({ queueChanged: true });
      void this.open(successorAfterRemove(index, next, this.queue.length));
    } else
      this.update({
        queueChanged: true,
        index: indexAfterRemove(this.state.index, index),
      });
    this.prepareNext();
  }
  clear() {
    this.cancelAudiobook();
    if (!this.persist()) return;
    this.observation.retire();
    this.community?.retire();
    this.openingEntry = null;
    this.durationChanged = null;
    this.endGuard.cancel();
    this.serverBound = null;
    this.cancelLookahead();
    this.lookahead?.closeOpening();
    this.feed.cancel();
    const count = this.queue.length;
    this.queue.clear();
    this.traversal.reset(this.queue.serials());
    this.selection++;
    log.info("queue.clear", { count });
    this.generation++;
    this.sources?.close();
    this.sources = null;
    this.media.pause();
    this.activeSource = null;
    this.media.removeAttribute("src");
    this.media.load();
    this.update({
      current: null,
      queueChanged: true,
      index: -1,
      playing: false,
      loading: false,
      position: 0,
      duration: 0,
    });
  }
  retire() {
    this.community?.retire();
    this.observation.retire();
    this.clear();
  }
}
