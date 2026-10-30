import type { Schema } from "../../../shared/schema";
import { createUuid } from "../../../shared/uuid";

import { HttpError } from "../../../shared/http";
import { log } from "../../../shared/log";

export type Source = Pick<Schema<"Source">, "id" | "ready"> &
  Partial<Pick<Schema<"Source">, "can_prepare">>;
export type SourcePreparer = {
  start(
    recording: string,
    source: string,
    body: Readonly<Schema<"PrepareSource">>,
    signal: AbortSignal,
  ): Promise<unknown>;
  status(
    recording: string,
    source: string,
    signal: AbortSignal,
  ): Promise<Schema<"SourcePreparationStatus"> | null>;
};
type SourceRequest =
  | { purpose: "cached" | "refresh" }
  | { purpose: "resolve"; body: Readonly<Schema<"ResolveSource">> };
export type SourceResolver = (
  recording: string,
  options: SourceRequest & { signal: AbortSignal },
) => Promise<Source[] | Schema<"SourceResolution">>;

export class PlaybackSources {
  private controller = new AbortController();
  private candidates: Source[] = [];
  private loaded = false;
  private refreshed = false;
  private resolved = false;
  private attempted = new Set<string>();
  private resolutionOutcome: Schema<"SourceResolution">["outcome"] | null =
    null;
  private waiting: Source | null = null;

  constructor(
    private recording: string,
    private resolve: SourceResolver,
    seed?: Source,
  ) {
    if (seed?.ready) {
      this.loaded = true;
      this.candidates = [{ ...seed }];
    }
  }

  get outcome(): Schema<"SourceResolution">["outcome"] | null {
    return this.resolutionOutcome;
  }

  get deferred(): Source | null {
    return this.waiting;
  }

  async reloadCached(): Promise<void> {
    if (this.controller.signal.aborted) return;
    await this.load({ purpose: "cached" });
    this.candidates = this.candidates.filter(
      (candidate) => !this.attempted.has(candidate.id),
    );
  }

  async next(): Promise<Source | undefined> {
    if (this.controller.signal.aborted) return;
    if (!this.loaded) {
      this.loaded = true;
      await this.load({ purpose: "cached" });
    }
    if (
      !this.candidates.length &&
      !this.refreshed &&
      !this.controller.signal.aborted
    ) {
      this.refreshed = true;
      try {
        await this.load({ purpose: "refresh" });
      } catch {
        if (this.controller.signal.aborted) return;
        log.warn("playback.source_refresh_failed", {
          recording_id: this.recording,
        });
        await this.load({ purpose: "cached" });
      }
    }
    if (
      !this.candidates.length &&
      !this.resolved &&
      !this.controller.signal.aborted
    ) {
      this.resolved = true;
      await this.findSource();
    }
    if (!this.controller.signal.aborted) {
      const source = this.candidates.shift();
      if (source) this.attempted.add(source.id);
      return source;
    }
  }

  async revalidate(source?: string): Promise<boolean> {
    if (this.controller.signal.aborted) return false;
    await this.load({ purpose: "cached" });
    if (this.controller.signal.aborted) return false;
    const retained = source !== undefined && this.candidates[0]?.id === source;
    this.candidates = this.candidates.filter(
      (candidate) => !this.attempted.has(candidate.id),
    );
    return retained;
  }

  private async findSource() {
    const body = Object.freeze({ request_id: createUuid() });
    log.info("playback.source_resolution_started", {
      recording_id: this.recording,
      request_id: body.request_id,
    });
    for (let attempt = 0; attempt < 2; attempt++) {
      try {
        await this.load({ purpose: "resolve", body });
        return;
      } catch (error) {
        if (this.controller.signal.aborted) return;
        if (
          attempt === 0 &&
          !(error instanceof HttpError && error.status < 500)
        ) {
          log.warn("playback.source_resolution_retry", {
            recording_id: this.recording,
            request_id: body.request_id,
          });
          continue;
        }
        log.warn("playback.source_resolution_failed", {
          recording_id: this.recording,
          request_id: body.request_id,
        });
        await this.load({ purpose: "cached" });
        this.candidates = this.candidates.filter(
          (source) => !this.attempted.has(source.id),
        );
        return;
      }
    }
  }

  private async load(request: SourceRequest) {
    const result = await this.resolve(this.recording, {
      ...request,
      signal: this.controller.signal,
    });
    if (this.controller.signal.aborted) return;
    const sources = Array.isArray(result) ? result : result.sources;
    if (!Array.isArray(result)) this.resolutionOutcome = result.outcome;
    const seen = new Set<string>();
    this.waiting =
      sources.find((source) => !source.ready && source.can_prepare === true) ??
      null;
    this.candidates = sources.filter((source) => {
      if (
        !source.ready ||
        seen.has(source.id) ||
        (request.purpose === "resolve" && this.attempted.has(source.id))
      )
        return false;
      seen.add(source.id);
      return true;
    });
    log.debug("playback.sources_loaded", {
      recording_id: this.recording,
      count: this.candidates.length,
      reason: request.purpose,
    });
  }

  close() {
    this.controller.abort();
    this.candidates = [];
  }
}
