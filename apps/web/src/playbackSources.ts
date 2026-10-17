import type { ResolveSource, Source as ApiSource } from "./types";
import { HttpError } from "./http";
import { log } from "./log";

export type Source = Pick<ApiSource, "id" | "ready">;
type SourceRequest =
  | { purpose: "cached" | "refresh" }
  | { purpose: "resolve"; body: Readonly<ResolveSource> };
export type SourceResolver = (
  recording: string,
  options: SourceRequest & { signal: AbortSignal },
) => Promise<Source[]>;

export class PlaybackSources {
  private controller = new AbortController();
  private candidates: Source[] = [];
  private loaded = false;
  private refreshed = false;
  private resolved = false;
  private attempted = new Set<string>();

  constructor(
    private recording: string,
    private resolve: SourceResolver,
  ) {}

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

  private async findSource() {
    const body = Object.freeze({ request_id: crypto.randomUUID() });
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
    const sources = await this.resolve(this.recording, {
      ...request,
      signal: this.controller.signal,
    });
    if (this.controller.signal.aborted) return;
    const seen = new Set<string>();
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
