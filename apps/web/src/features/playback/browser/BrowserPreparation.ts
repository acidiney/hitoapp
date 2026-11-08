import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import type { MediaPort } from "../model";
import {
  PlaybackSources,
  type Source,
  type SourcePreparer,
  type SourceResolver,
} from "../api/playbackSources";
import {
  prepareDeferredSource,
  type Submission,
} from "./BrowserDeferredSource";

const ACQUISITION_MS = 15_000;
const PROVIDER_PREPARATION_MS = 90_000;
const BOUNDARY_WAIT_MS = 10_000;

export type BrowserMediaOwner = {
  create(): MediaPort;
  release(media: MediaPort): void;
};

export type SubmissionLedger = {
  get(source: string): Submission | undefined;
  set(source: string, state: Submission): void;
};

export type PreparationTarget = {
  generation: number;
  revision: number;
  entry: string;
  recording: string;
};

export type Prepared = {
  sources: PlaybackSources;
  source?: Source;
};

type Buffering = "buffered" | "failed" | "timeout";

export class BrowserPreparation {
  readonly id = createUuid();
  readonly sources: PlaybackSources;
  private closed = false;
  private transferred = false;
  private media: MediaPort | null = null;
  private cancelBuffer: (() => void) | null = null;
  private found: Prepared | null = null;
  private rechecked = false;
  private cancelled!: (result: null) => void;
  private stopped: Promise<null>;
  private timer: ReturnType<typeof setTimeout>;
  private ready: Promise<Prepared | null>;
  private started = performance.now();
  private abort = new AbortController();
  private finding = true;
  private deadlineAt = 0;

  constructor(
    readonly target: PreparationTarget,
    resolve: SourceResolver,
    private owner: BrowserMediaOwner,
    seed?: Source,
    private preparer?: SourcePreparer,
    private ledger?: SubmissionLedger,
    fallbackAllowed?: () => Promise<boolean>,
  ) {
    this.sources = new PlaybackSources(
      target.recording,
      resolve,
      seed,
      fallbackAllowed,
    );
    this.stopped = new Promise<null>((resolve) => {
      this.cancelled = resolve;
    });
    this.deadlineAt = this.started + ACQUISITION_MS;
    this.timer = setTimeout(() => this.close("deadline"), ACQUISITION_MS);
    this.ready = Promise.race([this.prepare(), this.stopped]);
    log.debug("playback.next_preparation_started", {
      recording_id: target.recording,
      operation_id: this.id,
    });
  }

  private async prepare(): Promise<Prepared | null> {
    try {
      let source = await this.sources.next();
      if (!source && !this.closed) source = await this.prepareDeferred();
      if (this.closed) return null;
      if (!source) {
        this.finding = false;
        clearTimeout(this.timer);
        this.finished("unavailable");
        return { sources: this.sources };
      }
      this.finding = false;
      clearTimeout(this.timer);
      const prepared: Prepared = { sources: this.sources, source };
      this.found = prepared;
      const media = this.owner.create();
      this.media = media;
      const outcome = await new Promise<Buffering>((resolve) => {
        const finish = (result: Buffering) => {
          media.removeEventListener("canplay", readyEvent);
          media.removeEventListener("error", failedEvent);
          this.cancelBuffer = null;
          resolve(result);
        };
        const readyEvent = () => finish("buffered");
        const failedEvent = () => finish("failed");
        this.cancelBuffer = failedEvent;
        this.timer = setTimeout(() => finish("timeout"), ACQUISITION_MS);
        media.addEventListener("canplay", readyEvent, { once: true });
        media.addEventListener("error", failedEvent, { once: true });
        media.src = `/api/v1/sources/${encodeURIComponent(source.id)}/audio`;
        media.load();
      });
      if (this.closed) return null;
      if (this.transferred) return this.found;
      clearTimeout(this.timer);
      if (outcome === "failed") {
        this.found = null;
        this.releaseMedia();
        this.finished("audio_unavailable");
        return { sources: this.sources };
      }
      if (outcome === "timeout") {
        this.releaseMedia();
        this.finished("buffer_timeout");
        return this.found;
      }
      this.finished("buffered");
      return this.found;
    } catch {
      if (this.closed) return null;
      clearTimeout(this.timer);
      this.cancelBuffer?.();
      this.releaseMedia();
      this.finished("failed");
      return { sources: this.sources };
    }
  }

  private async prepareDeferred(): Promise<Source | undefined> {
    const waiting = this.sources.deferred;
    if (!this.preparer || !waiting || this.abort.signal.aborted) return;
    clearTimeout(this.timer);
    this.deadlineAt = this.started + PROVIDER_PREPARATION_MS;
    this.timer = setTimeout(
      () => this.close("deadline"),
      Math.max(0, this.deadlineAt - performance.now()),
    );
    const ready = await prepareDeferredSource(
      this.preparer,
      this.target.recording,
      waiting.id,
      this.abort.signal,
      { remainingMs: () => this.deadlineAt - performance.now() },
      this.ledger?.get(waiting.id),
      (state) => this.ledger?.set(waiting.id, state),
    );
    if (!ready || this.closed) return;
    await this.sources.reloadCached();
    return this.sources.next();
  }

  async read(): Promise<Prepared | null> {
    const prepared = await this.settled();
    if (this.closed || !prepared) return null;
    const started = performance.now();
    try {
      const retained = await Promise.race([
        this.sources.revalidate(prepared.source?.id),
        this.stopped,
      ]);
      if (this.closed) return null;
      log.debug("playback.next_preparation_checked", {
        recording_id: this.target.recording,
        operation_id: this.id,
        reason: retained ? "retained" : "changed",
        duration_ms: Math.round(performance.now() - started),
      });
      if (!retained) {
        this.releaseMedia();
        return { sources: this.sources };
      }
    } catch (error) {
      this.close();
      throw error;
    }
    return prepared;
  }

  private async settled(): Promise<Prepared | null> {
    if (!this.finding) return this.ready;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const limit = new Promise<"waited">((resolve) => {
      timer = setTimeout(() => resolve("waited"), BOUNDARY_WAIT_MS);
    });
    const outcome = await Promise.race([this.ready, limit]);
    clearTimeout(timer);
    if (outcome !== "waited") return outcome;
    if (this.finding) {
      log.debug("playback.next_preparation_finished", {
        recording_id: this.target.recording,
        operation_id: this.id,
        reason: "still_finding_source",
        duration_ms: Math.round(performance.now() - this.started),
      });
      this.sources.close();
      this.abort.abort();
      return { sources: this.sources };
    }
    return this.ready;
  }

  readyNow(): Prepared | null {
    return this.closed || this.transferred ? null : this.found;
  }

  recheck(changed: () => void) {
    const source = this.found?.source;
    if (!source || this.rechecked || this.closed || this.transferred) return;
    this.rechecked = true;
    this.sources.revalidate(source.id).then(
      (retained) => {
        if (this.closed || this.transferred || retained) return;
        this.found = null;
        changed();
      },
      () => {},
    );
  }

  transfer() {
    if (this.closed) return false;
    this.transferred = true;
    clearTimeout(this.timer);
    this.cancelBuffer?.();
    this.releaseMedia();
    return true;
  }

  close(reason: "superseded" | "deadline" = "superseded") {
    if (this.closed || this.transferred) return;
    this.closed = true;
    clearTimeout(this.timer);
    this.sources.close();
    this.abort.abort();
    this.cancelBuffer?.();
    this.releaseMedia();
    this.cancelled(null);
    this.finished(reason);
  }

  private releaseMedia() {
    const media = this.media;
    this.media = null;
    if (!media) return;
    media.pause();
    media.removeAttribute("src");
    media.load();
    this.owner.release(media);
  }

  private finished(outcome: string) {
    log.debug("playback.next_preparation_finished", {
      recording_id: this.target.recording,
      operation_id: this.id,
      reason: outcome,
      duration_ms: Math.round(performance.now() - this.started),
    });
  }
}
