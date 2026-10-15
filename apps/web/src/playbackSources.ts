import type { Source as ApiSource } from "./types";
import { log } from "./log";

export type Source = Pick<ApiSource, "id" | "ready">;
export type SourceResolver = (
  recording: string,
  options: { refresh: boolean; signal: AbortSignal },
) => Promise<Source[]>;

export class PlaybackSources {
  private controller = new AbortController();
  private candidates: Source[] = [];
  private loaded = false;
  private refreshed = false;

  constructor(
    private recording: string,
    private resolve: SourceResolver,
  ) {}

  async next(): Promise<Source | undefined> {
    if (this.controller.signal.aborted) return;
    if (!this.loaded) {
      this.loaded = true;
      await this.load(false);
    }
    if (
      !this.candidates.length &&
      !this.refreshed &&
      !this.controller.signal.aborted
    ) {
      this.refreshed = true;
      try {
        await this.load(true);
      } catch {
        if (this.controller.signal.aborted) return;
        log.warn("playback.source_refresh_failed", {
          recording_id: this.recording,
        });
        await this.load(false);
      }
    }
    if (!this.controller.signal.aborted) return this.candidates.shift();
  }

  private async load(refresh: boolean) {
    const sources = await this.resolve(this.recording, {
      refresh,
      signal: this.controller.signal,
    });
    if (this.controller.signal.aborted) return;
    const seen = new Set<string>();
    this.candidates = sources.filter((source) => {
      if (!source.ready || seen.has(source.id)) return false;
      seen.add(source.id);
      return true;
    });
    log.debug("playback.sources_loaded", {
      recording_id: this.recording,
      count: this.candidates.length,
      reason: refresh ? "refreshed" : "cached",
    });
  }

  close() {
    this.controller.abort();
    this.candidates = [];
  }
}
