import { rememberQueuedByYou } from "./queuedByYou";
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
  refusal,
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
import { BrowserStream } from "./browser/BrowserStream";
import { BrowserAcquisition } from "./browser/BrowserAcquisition";
import {
  streamingQuality,
  subscribeStreamingQuality,
} from "./browser/StreamingQuality";
export type { Source } from "./api/playbackSources";

const NEXT_RECHECK_SECONDS = 20;
const RETRY_RESET_SECONDS = 10;

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
  private acquisition: BrowserAcquisition | null = null;
  private heardAfter = 0;
  private blockedUnheard = false;
  private generation = 0;
  private selection = 0;
  private sourceAttempt = 0;
  private pendingSeek: number | null = null;
  private activeSource: Source | null = null;
  private durationChanged: (() => void) | null = null;
  private serverBound: number | null = null;
  private entryEnded = false;
  private hiddenRetries = 0;
  private unavailableRecording: string | null = null;
  private retryPosition = 0;
  private restored = false;
  private formatProbe: Promise<boolean> = Promise.resolve(false);
  private endGuard = new BrowserEndGuard(() => this.endAtKnownDuration());
  private lookahead: BrowserLookahead | null;
  private stopListening: () => void = () => {};
  private progress: BrowserProgressGuard;
  private audiobook: BrowserAudiobookRun;
  private media: BrowserStream;
  private stopQuality: () => void = () => {};
  private feed = new QueueFeed(
    () => this.store.setPending(this.feed.pending),
    (error) => this.update({ error }),
  );

  constructor(
    media: MediaPort,
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
    this.media = new BrowserStream(media);
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
      (message) => this.progressBlocked(message),
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

  activate() {
    this.media.activate();
    this.stopQuality();
    if (!this.lookahead) return;
    let quality = streamingQuality();
    this.stopQuality = subscribeStreamingQuality(() => {
      const selected = streamingQuality();
      if (selected === quality) return;
      quality = selected;
      this.cancelLookahead();
      this.prepareNext();
    });
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
        if (
          this.state.desiredPlaying === true &&
          this.activeSource &&
          !media.paused &&
          !media.seeking &&
          media.currentTime > this.heardAfter
        )
          this.heard();
        if (this.activeSource && !media.paused && media.currentTime > 0) {
          this.traversal.resetFailures();
          this.prepareNext();
          this.recheckNext();
          if (
            this.hiddenRetries > 0 &&
            media.currentTime > this.retryPosition + RETRY_RESET_SECONDS
          )
            this.hiddenRetries = 0;
        }
        this.update({ position: this.pendingSeek ?? media.currentTime });
        this.progress.observe();
        if (this.activeSource) this.endGuard.observe(media, this.serverBound);
      },
      play: () => {
        if (!this.activeSource || this.blockedUnheard) {
          log.debug("playback.released_play_ignored", {
            recording_id: this.state.current?.id,
          });
          media.pause();
          return;
        }
        log.info("playback.started", { recording_id: this.state.current?.id });
        const recovered =
          this.unavailableRecording !== null &&
          this.unavailableRecording === this.state.current?.id;
        if (recovered) this.unavailableRecording = null;
        this.update({
          playing: true,
          loading: false,
          ...(recovered ? { error: "" } : {}),
        });
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
          if (rejected || !this.retryInPlace()) void this.recover();
        }
      },
      stalled: () => {
        if (!media.paused && (media.readyState ?? 4) < 3) this.retryInPlace();
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

  private retryInPlace() {
    const source = this.activeSource;
    if (
      !source ||
      document.visibilityState !== "hidden" ||
      this.hiddenRetries >= 1
    )
      return false;
    const acquisition = this.acquisition;
    if (acquisition?.overdue) {
      acquisition.expireNow();
      return true;
    }
    this.hiddenRetries++;
    const position =
      this.media.currentTime > 0 ? this.media.currentTime : this.state.position;
    this.retryPosition = position;
    this.pendingSeek =
      this.pendingSeek === 0 ? 0 : position > 0 ? position : null;
    log.warn("playback.recovering_in_place", {
      recording_id: this.state.current?.id,
    });
    this.openSourceAudio(source, position);
    void this.playSource(this.generation, ++this.sourceAttempt);
    return true;
  }

  private recheckNext() {
    const remaining = this.state.duration - this.media.currentTime;
    if (this.state.duration > 0 && remaining <= NEXT_RECHECK_SECONDS)
      this.lookahead?.recheck(() => {
        this.cancelLookahead();
        this.prepareNext();
      });
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

  setRecordingLiked(recording: string, liked: boolean) {
    const update = (track: Track): Track =>
      track.id === recording ? { ...track, liked } : track;
    log.info("playback.metadata_updated", {
      recording_id: recording,
      action: "liked",
    });
    this.queue.updateRecording(update);
    this.update({
      queueChanged: true,
      current: this.state.current ? update(this.state.current) : null,
    });
  }

  restoreQueue(queue: PlaybackQueueInput[], index: number, position: number) {
    if (this.queue.length > 0 || !queue[index]) return false;
    this.queue.replace(queue);
    this.traversal.reset(this.queue.serials(), this.queue.serialAt(index));
    const current = this.queue.at(index).recording;
    this.restored = true;
    log.info("queue.restored", { count: queue.length, index });
    this.update({
      queueChanged: true,
      current,
      index,
      position,
      duration: current.duration_ms / 1000,
      playing: false,
      loading: false,
      error: "",
    });
    return true;
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

  async addToQueue(track: Track): Promise<void> {
    this.cancelAudiobook();
    if (this.queue.length === 0) {
      await this.playQueue([track]);
      return;
    }
    this.appendBatch([track]);
  }

  async playNext(track: Track): Promise<void> {
    this.cancelAudiobook();
    if (this.queue.length === 0) {
      await this.playQueue([track]);
      return;
    }
    if (!this.appendBatch([track])) return;
    const added = this.queue.at(this.queue.length - 1).id;
    const following = this.queue.at(this.state.index + 1);
    if (following && following.id !== added)
      this.moveEntry(added, following.id);
    rememberQueuedByYou(added);
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
    natural = false,
  ) {
    this.activate();
    if (!continuation) {
      this.cancelAudiobook();
      if (!this.persist()) return;
    }
    this.restored = false;
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
    const instant =
      natural && !continuation ? (prepared?.readyNow() ?? null) : null;
    const generation = ++this.generation;
    this.hiddenRetries = 0;
    this.acquisition?.stop();
    this.acquisition = occurrence ? this.acquire(occurrence) : null;
    this.blockedUnheard = false;
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
    if (!continuation && !instant) this.media.pause();
    this.activeSource = null;
    this.formatProbe = Promise.resolve(false);
    if (!instant) {
      this.media.removeAttribute("src");
      this.media.load();
    }
    if (continuation) this.applySpeed(continuation.speed);
    this.pendingSeek = position > 0 ? position : null;
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
      desiredPlaying: current ? true : undefined,
      retrying: null,
      duration: (current?.duration_ms ?? 0) / 1000,
    });
    if (!current || !occurrence) {
      this.update({ loading: false });
      return;
    }
    try {
      if (prepared) {
        const buffered = instant ?? (await prepared.read());
        if (
          generation !== this.generation ||
          !this.lookahead?.isOpening(prepared)
        ) {
          prepared.close();
          return;
        }
        const acquisition = this.acquisition;
        if (acquisition?.overdue) {
          this.lookahead?.closeOpening();
          acquisition.expireNow();
          return;
        }
        this.lookahead?.settleOpening();
        if (buffered?.ended) {
          log.info("playback.acquisition_final", {
            recording_id: current.id,
            reason: buffered.ended,
          });
          this.acquisition?.finalPass();
        }
        if (buffered && prepared.transfer()) {
          if (buffered.source) {
            this.startPrepared(occurrence, buffered.source, prepared.id);
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
    } catch (error) {
      log.warn("playback.resolution_failed", { recording_id: current.id });
      if (generation === this.generation)
        await this.exhausted(this.sources, error);
    }
  }

  private acquire(occurrence: PlaybackOccurrence) {
    const acquisition: BrowserAcquisition = new BrowserAcquisition(
      occurrence.id,
      () => this.expire(acquisition),
    );
    return acquisition;
  }

  private heard() {
    const acquisition = this.acquisition;
    if (acquisition?.overdue) {
      acquisition.expireNow();
      return;
    }
    this.acquisition = null;
    acquisition?.stop();
    if (acquisition)
      log.info("playback.acquisition_heard", {
        recording_id: this.state.current?.id,
        index: acquisition.attempt,
        duration_ms: acquisition.elapsedMs,
      });
    this.update({ retrying: null, desiredPlaying: undefined });
  }

  private expire(acquisition: BrowserAcquisition) {
    if (acquisition !== this.acquisition) return;
    this.acquisition = null;
    log.warn("playback.acquisition_expired", {
      recording_id: this.state.current?.id,
      index: acquisition.attempt,
      duration_ms: acquisition.elapsedMs,
    });
    this.generation++;
    this.sourceAttempt++;
    this.sources?.close();
    this.releaseMedia();
    void this.skipFailed(false);
  }

  private releaseMedia() {
    this.media.pause();
    this.activeSource = null;
    this.media.removeAttribute("src");
    this.media.load();
  }

  private async exhausted(sources: PlaybackSources | null, error?: unknown) {
    const generation = this.generation;
    const acquisition = this.acquisition;
    const current = this.state.current;
    const formatRejected = await this.formatProbe;
    if (
      generation !== this.generation ||
      acquisition !== this.acquisition ||
      sources !== this.sources ||
      current?.id !== this.state.current?.id
    )
      return;
    if (acquisition?.overdue) {
      acquisition.expireNow();
      return;
    }
    const pass =
      acquisition &&
      current &&
      sources &&
      !refusal(error) &&
      sources.retryable &&
      !formatRejected
        ? acquisition.nextPass()
        : null;
    if (!pass || !acquisition || !current || !sources) {
      await this.skipFailed(formatRejected);
      return;
    }
    log.info("playback.acquisition_retry", {
      recording_id: current.id,
      index: pass.attempt,
      count: pass.of,
      reason: sources.outcome ?? "empty",
      duration_ms: acquisition.elapsedMs,
    });
    this.update({ retrying: pass, error: "", loading: true });
    const continued = await acquisition.waitForPass();
    if (
      !continued ||
      generation !== this.generation ||
      acquisition !== this.acquisition
    )
      return;
    if (acquisition.overdue) {
      acquisition.expireNow();
      return;
    }
    sources.close();
    this.sources = new PlaybackSources(
      current.id,
      this.resolve,
      undefined,
      this.fallbackAllowed,
    );
    const replacement = this.sources;
    try {
      await this.useNextSource(this.pendingSeek ?? this.state.position);
    } catch (failure) {
      if (generation === this.generation)
        await this.exhausted(replacement, failure);
    }
  }

  private progressBlocked(message: string) {
    const acquisition = this.acquisition;
    if (!acquisition) {
      this.update({ error: message });
      return;
    }
    this.acquisition = null;
    acquisition.stop();
    this.sourceAttempt++;
    this.blockedUnheard = true;
    log.info("playback.acquisition_paused", {
      recording_id: this.state.current?.id,
      index: acquisition.attempt,
      duration_ms: acquisition.elapsedMs,
      reason: "progress_blocked",
    });
    this.update({
      error: message,
      retrying: null,
      desiredPlaying: undefined,
      playing: false,
      loading: false,
    });
  }

  private stopIntendedPlayback() {
    const acquisition = this.acquisition;
    this.acquisition = null;
    acquisition?.stop();
    log.info("playback.acquisition_paused", {
      recording_id: this.state.current?.id,
      index: acquisition?.attempt,
      duration_ms: acquisition?.elapsedMs,
      reason: this.activeSource ? "granted" : "searching",
    });
    const position = this.pendingSeek ?? this.state.position;
    this.generation++;
    this.sourceAttempt++;
    this.sources?.close();
    this.sources = null;
    this.releaseMedia();
    this.pendingSeek = null;
    this.restored = true;
    this.update({
      position,
      playing: false,
      loading: false,
      desiredPlaying: false,
      retrying: null,
    });
  }

  private startPrepared(
    occurrence: PlaybackOccurrence,
    source: Source,
    operation: string,
  ) {
    this.openSourceAudio(source, 0);
    this.activeSource = { ...source };
    this.observeSource(occurrence, source);
    this.entryEnded = false;
    log.debug("playback.next_preparation_adopted", {
      recording_id: occurrence.recording.id,
      operation_id: operation,
      source_id: source.id,
    });
  }

  private async useNextSource(position: number) {
    const generation = this.generation;
    const attempt = ++this.sourceAttempt;
    const sources = this.sources;
    const entry = this.openingEntry;
    if (!entry) return;
    const pending = this.acquisition;
    if (pending?.overdue) {
      pending.expireNow();
      return;
    }
    const source = await sources?.next();
    if (
      generation !== this.generation ||
      attempt !== this.sourceAttempt ||
      sources !== this.sources
    )
      return;
    if (!source) {
      await this.exhausted(sources ?? null);
      return;
    }
    const acquisition = this.acquisition;
    if (acquisition?.overdue) {
      acquisition.expireNow();
      return;
    }
    this.pendingSeek ??= position > 0 ? position : null;
    this.activeSource = { ...source };
    log.info("playback.source_changed", {
      recording_id: this.state.current?.id,
      source_id: source.id,
      position_ms: Math.floor(position * 1000),
    });
    this.openSourceAudio(source, position);
    this.observeSource(entry, source);
    await this.playSource(generation, attempt);
  }

  private openSourceAudio(source: Source, position: number) {
    const start = this.pendingSeek ?? position;
    this.pendingSeek = null;
    this.heardAfter = start;
    const generation = this.generation;
    this.media.openSource(source.id, start, this.state.duration, () =>
      generation === this.generation && this.acquisition
        ? this.acquisition.negotiationBudgetMs()
        : Number.POSITIVE_INFINITY,
    );
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
      if (error instanceof Error && error.name === "NotAllowedError") {
        if (this.state.desiredPlaying === true) this.stopIntendedPlayback();
        this.update({ loading: false, error: "Press play to start audio." });
      } else {
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
    this.pendingSeek =
      this.pendingSeek === 0 ? 0 : position > 0 ? position : null;
    const intended = this.state.desiredPlaying ?? this.state.playing;
    this.media.pause();
    this.update({
      playing: false,
      loading: true,
      ...(intended ? { desiredPlaying: true } : {}),
    });
    log.warn("playback.recovering", {
      recording_id: this.state.current?.id,
    });
    try {
      await this.useNextSource(position);
    } catch (error) {
      if (generation === this.generation && sources === this.sources)
        await this.exhausted(sources, error);
    }
  }

  private probeFormat(source: Source) {
    this.formatProbe = sourceWasDelivered(
      this.media.src ||
        `/api/v1/sources/${encodeURIComponent(source.id)}/audio`,
    );
  }

  private async skipFailed(known?: boolean) {
    this.acquisition?.stop();
    this.acquisition = null;
    const generation = this.generation;
    const formatRejected = known ?? (await this.formatProbe);
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
    this.unavailableRecording = this.state.current?.id ?? null;
    this.update({
      error: providerUnavailable
        ? "An audio service is unavailable. Select the track again to retry."
        : formatRejected
          ? UNSUPPORTED_FORMAT_MESSAGE
          : `Audio is unavailable for ${this.state.current?.title ?? "this track"}.`,
      loading: false,
      retrying: null,
      desiredPlaying: undefined,
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
    await this.setPlaying(!(this.state.desiredPlaying ?? !this.media.paused));
  }
  async setPlaying(playing: boolean) {
    this.cancelAudiobook();
    if (this.state.desiredPlaying === true) {
      if (!playing) this.stopIntendedPlayback();
      return;
    }
    if (this.state.loading) return;
    if (this.restored) {
      if (playing) await this.open(this.state.index, this.state.position);
      return;
    }
    log.info("playback.playing_set", {
      recording_id: this.state.current?.id,
      enabled: playing,
    });
    if (playing === !this.media.paused) return;
    const generation = this.generation;
    const attempt = ++this.sourceAttempt;
    if (playing && this.blockedUnheard && this.openingEntry) {
      this.blockedUnheard = false;
      this.heardAfter = this.media.currentTime;
      this.acquisition = this.acquire(this.openingEntry);
      this.update({ desiredPlaying: true });
    }
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
    if (this.restored) {
      this.update({ position });
      return;
    }
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
    if (this.acquisition) this.heardAfter = position;
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
    else
      await this.open(
        this.traversal.previous(this.queue.serials(), this.state.index),
      );
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
    await this.open(index ?? this.queue.length, 0, undefined, ended);
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
    this.restored = false;
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
    this.acquisition?.stop();
    this.acquisition = null;
    this.blockedUnheard = false;
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
      desiredPlaying: undefined,
      retrying: null,
      position: 0,
      duration: 0,
    });
  }
  retire() {
    this.stopQuality();
    this.community?.retire();
    this.observation.retire();
    this.clear();
    this.media.close();
  }
}
