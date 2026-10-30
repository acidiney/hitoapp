import type { Schema } from "../../../shared/schema";

import type { MediaPort, PlaybackOccurrence, Track } from "../model";

export type BrowserCheckpoint = Readonly<{
  recording: Track;
  position: number;
  speed: number;
  chapter?: Schema<"AudiobookChapterAnchor">;
}>;
type Episode = {
  entry: PlaybackOccurrence;
  media: MediaPort;
  source: string;
  current(): boolean;
  ready: boolean;
  seek: number | null;
  qualified: boolean;
  admitted: number | null;
};

export class BrowserObservation {
  private episode: Episode | null = null;
  private pending: BrowserCheckpoint | null = null;

  constructor(private admit: (sample: BrowserCheckpoint) => boolean) {}

  begin(
    entry: PlaybackOccurrence,
    media: MediaPort,
    source: string,
    current: () => boolean,
  ) {
    if (this.pending)
      throw new Error(
        "The heard checkpoint must be admitted before changing source.",
      );
    this.episode = {
      entry,
      media,
      source,
      current,
      ready: (media.readyState ?? 0) >= 2,
      seek: null,
      qualified: false,
      admitted: null,
    };
  }
  metadata() {
    if (this.current()) this.episode!.ready = true;
  }
  seeking(position: number) {
    if (this.current()) this.episode!.seek = position;
  }
  seeked() {
    const episode = this.episode;
    if (
      episode &&
      this.current() &&
      !episode.media.seeking &&
      episode.seek !== null
    )
      episode.seek = null;
  }
  observe() {
    const episode = this.episode;
    if (
      episode &&
      !episode.entry.chapter &&
      this.current() &&
      Math.abs(episode.media.currentTime - (episode.admitted ?? 0)) >= 10
    )
      return this.persist();
    if (
      !episode ||
      !this.current() ||
      !episode.ready ||
      (episode.media.readyState !== undefined &&
        episode.media.readyState < 2) ||
      episode.seek !== null ||
      episode.media.seeking ||
      episode.media.paused ||
      episode.media.ended
    )
      return true;
    episode.qualified = true;
    const position = episode.media.currentTime;
    if (
      (episode.entry.chapter && episode.admitted === null) ||
      Math.abs(position - (episode.admitted ?? 0)) >= 10
    )
      return this.persist();
    return true;
  }
  persist() {
    if (!this.retry()) return false;
    const episode = this.episode;
    if (!episode || !this.current()) return true;
    const position = episode.media.currentTime;
    if (
      !Number.isFinite(position) ||
      position < 0 ||
      episode.seek !== null ||
      episode.media.seeking ||
      (episode.entry.chapter && !episode.qualified) ||
      (!episode.entry.chapter && position === 0) ||
      episode.admitted === position
    )
      return true;
    this.pending = Object.freeze({
      recording: episode.entry.recording,
      position,
      speed: episode.media.playbackRate,
      ...(episode.entry.chapter ? { chapter: episode.entry.chapter } : {}),
    });
    return this.retry();
  }
  retry() {
    if (!this.pending) return true;
    if (!this.admit(this.pending)) return false;
    if (this.episode) this.episode.admitted = this.pending.position;
    this.pending = null;
    return true;
  }
  retire() {
    this.episode = null;
    this.pending = null;
  }
  private current() {
    const episode = this.episode;
    if (!episode?.current()) return false;
    const source = episode.media.currentSrc || episode.media.src;
    return new URL(source, "http://hito.invalid").pathname === episode.source;
  }
}
