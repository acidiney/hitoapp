import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import type { MediaPort } from "../model";
import {
  PlaybackSources,
  type Source,
  type SourcePreparer,
  type SourceResolver,
} from "../api/playbackSources";
import { prepareDeferredSource } from "./BrowserDeferredSource";

const ACQUISITION_MS = 15_000;
const PROVIDER_PREPARATION_MS = 90_000;
const BOUNDARY_WAIT_MS = 10_000;

export type BrowserMediaOwner = {
  create(): MediaPort;
  activate(media: MediaPort): void;
  release(media: MediaPort): void;
};

export type PreparationTarget = {
  generation: number;
  revision: number;
  entry: string;
  recording: string;
};

type Prepared = {
  sources: PlaybackSources;
  media?: MediaPort;
  source?: Source;
};

export class BrowserPreparation {
  readonly id = createUuid();
  readonly sources: PlaybackSources;
  private closed = false;
  private transferred = false;
  private media: MediaPort | null = null;
  private cancelBuffer: (() => void) | null = null;
  private cancelled!: (result: null) => void;
  private stopped: Promise<null>;
  private timer: ReturnType<typeof setTimeout>;
  private ready: Promise<Prepared | null>;
  private started = performance.now();
  private abort = new AbortController();
  private provider = false;

  constructor(
    readonly target: PreparationTarget,
    resolve: SourceResolver,
    private owner: BrowserMediaOwner,
    seed?: Source,
    private preparer?: SourcePreparer,
  ) {
    this.sources = new PlaybackSources(target.recording, resolve, seed);
    this.stopped = new Promise<null>((resolve) => {
      this.cancelled = resolve;
    });
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
        clearTimeout(this.timer);
        this.finished("unavailable");
        return { sources: this.sources };
      }
      const media = this.owner.create();
      this.media = media;
      const buffered = await new Promise<boolean>((resolve) => {
        const finish = (ready: boolean) => {
          media.removeEventListener("canplay", readyEvent);
          media.removeEventListener("error", failedEvent);
          this.cancelBuffer = null;
          resolve(ready);
        };
        const readyEvent = () => finish(true);
        const failedEvent = () => finish(false);
        this.cancelBuffer = failedEvent;
        media.addEventListener("canplay", readyEvent, { once: true });
        media.addEventListener("error", failedEvent, { once: true });
        media.src = `/api/v1/sources/${encodeURIComponent(source.id)}/audio`;
        media.load();
      });
      if (this.closed) return null;
      clearTimeout(this.timer);
      if (!buffered) {
        this.releaseMedia();
        this.finished("audio_unavailable");
        return { sources: this.sources };
      }
      this.finished("buffered");
      return { sources: this.sources, media, source };
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
    if (!this.preparer || !waiting) return;
    clearTimeout(this.timer);
    this.timer = setTimeout(
      () => this.close("deadline"),
      Math.max(0, PROVIDER_PREPARATION_MS - (performance.now() - this.started)),
    );
    this.provider = true;
    try {
      const ready = await prepareDeferredSource(
        this.preparer,
        this.target.recording,
        waiting.id,
        this.abort.signal,
      );
      if (!ready || this.closed) return;
      await this.sources.reloadCached();
      return this.sources.next();
    } finally {
      this.provider = false;
    }
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
    if (!this.provider) return this.ready;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const limit = new Promise<"waited">((resolve) => {
      timer = setTimeout(() => resolve("waited"), BOUNDARY_WAIT_MS);
    });
    const outcome = await Promise.race([this.ready, limit]);
    clearTimeout(timer);
    if (outcome !== "waited") return outcome;
    log.debug("playback.next_preparation_finished", {
      recording_id: this.target.recording,
      operation_id: this.id,
      reason: "provider_still_preparing",
      duration_ms: Math.round(performance.now() - this.started),
    });
    this.abort.abort();
    return this.ready;
  }

  transfer() {
    if (this.closed) return false;
    this.transferred = true;
    this.media = null;
    clearTimeout(this.timer);
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
