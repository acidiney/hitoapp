import type { MediaPort } from "../model";
import type { SuccessfulPlayback } from "../../../shared/types";
import { createUuid } from "../../../shared/uuid";
import { log } from "../../../shared/log";

type Sample = { at: number; position: number; speed: number };
type Episode = {
  media: MediaPort;
  source: string;
  current(): boolean;
  heard: number;
  seeking: boolean;
  completed: boolean;
  previous: Sample | null;
};

export class BrowserCommunityObservation {
  private episode: Episode | null = null;

  constructor(
    private admit: (source: string, claim: SuccessfulPlayback) => void,
  ) {}

  begin(media: MediaPort, source: string, current: () => boolean) {
    this.episode = {
      media,
      source,
      current,
      heard: 0,
      seeking: false,
      completed: false,
      previous: null,
    };
  }

  reset() {
    if (this.episode) this.episode.previous = null;
  }

  seeking() {
    this.reset();
    if (this.episode) this.episode.seeking = true;
  }

  seeked() {
    this.reset();
    if (this.episode) this.episode.seeking = false;
  }

  observe() {
    const episode = this.episode;
    if (!episode || episode.completed) return;
    const media = episode.media;
    const sample = {
      at: performance.now(),
      position: media.currentTime,
      speed: media.playbackRate,
    };
    if (
      !episode.current() ||
      episode.seeking ||
      media.seeking ||
      media.paused ||
      media.ended ||
      (media.readyState ?? 0) < 2 ||
      !Number.isFinite(sample.at) ||
      sample.at < 0 ||
      !Number.isFinite(sample.position) ||
      sample.position < 0 ||
      !Number.isFinite(sample.speed) ||
      sample.speed <= 0 ||
      sample.speed > 4
    ) {
      this.reset();
      return;
    }
    const previous = episode.previous;
    episode.previous = sample;
    if (!previous) return;
    const elapsed = (sample.at - previous.at) / 1000;
    const advanced = sample.position - previous.position;
    if (elapsed < 0) {
      this.reset();
      return;
    }
    if (
      elapsed <= 0 ||
      advanced <= 0 ||
      previous.speed !== sample.speed ||
      advanced > elapsed * sample.speed + 0.05
    )
      return;
    episode.heard += Math.min(advanced, elapsed * sample.speed);
    if (episode.heard < 5) return;
    episode.completed = true;
    try {
      this.admit(episode.source, { submission_id: createUuid() });
    } catch {
      log.debug("community_playback.observation_dropped", {
        reason: "admission_failed",
      });
    }
  }

  retire() {
    this.episode = null;
  }
}
