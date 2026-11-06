import type { Schema } from "../../shared/schema";

export type Track = Pick<
  Schema<"Recording">,
  "id" | "title" | "artist" | "album" | "duration_ms" | "available"
> &
  Partial<
    Pick<
      Schema<"Recording">,
      | "inherited_title"
      | "inherited_artist"
      | "release_id"
      | "artist_id"
      | "liked"
      | "plays_from_services"
    >
  >;

export type PlaybackStart = { kind: "saved" | "position"; position: number };
export type PlaybackQueueInput =
  | Track
  | { recording: Track; chapter?: Schema<"AudiobookChapterAnchor"> };
export type PlaybackOccurrence = {
  id: string;
  recording: Track;
  chapter?: Schema<"AudiobookChapterAnchor"> | null;
};
export const MAX_QUEUE_ENTRIES = 100_000;
export type QueueSupply = {
  remaining(): number;
  next(signal: AbortSignal): Promise<PlaybackQueueInput[]>;
};
export type PlaybackSelection = {
  queue: PlaybackQueueInput[];
  index?: number;
  start?: PlaybackStart;
  more?: QueueSupply;
};
export type PreparedSelection = (
  selection: Track | PlaybackSelection,
) => Promise<boolean>;

export type SpeedRange = { min: number; max: number };

export type PlaybackView = {
  current: Track | null;
  playing: boolean;
  desiredPlaying?: boolean;
  canPrevious?: boolean;
  loading: boolean;
  error: string;
  repeat: "off" | "all" | "one";
  shuffle: boolean;
  speed: number;
  effectiveSpeed?: number;
  speedRange?: SpeedRange | null;
  volume: number;
  connection: "connecting" | "ready" | "disconnected";
  controlsAvailable?: boolean;
  resumeFailed?: boolean;
};
export type PlaybackPosition = { position: number; duration: number };
export type QueueView = {
  version: number;
  total: number;
  currentIndex: number | null;
  currentEntry: string | null;
  pending: number;
};
export type QueuePage = {
  version: number;
  total: number;
  offset: number;
  entries: PlaybackOccurrence[];
};

export interface Player {
  readonly mode: "browser" | "native";
  readonly supports: {
    previous: boolean;
    shuffle: boolean;
    repeat: boolean;
    speed: boolean;
  };
  snapshot(): PlaybackView;
  subscribe(listener: () => void): () => void;
  positionSnapshot(): PlaybackPosition;
  subscribePosition(listener: () => void): () => void;
  queueSnapshot(): QueueView;
  queuePage(
    offset: number,
    limit: number,
    signal?: AbortSignal,
  ): Promise<QueuePage>;
  selectEntry(entry: string): Promise<void>;
  removeEntry(entry: string): void;
  moveEntry(entry: string, before: string | null): void;
  playQueue(
    queue: PlaybackQueueInput[],
    index?: number,
    start?: PlaybackStart,
    more?: QueueSupply,
  ): Promise<void>;
  addToQueue?(track: Track): Promise<void>;
  playNext?(track: Track): Promise<void>;
  continueAudiobook(audiobook: string, signal: AbortSignal): Promise<boolean>;
  prepareSelection(): PreparedSelection;
  toggle(): Promise<void>;
  setPlaying(playing: boolean): Promise<void>;
  previous(): Promise<void>;
  next(): Promise<void>;
  seek(seconds: number): void;
  volume(value: number): void;
  speed(value: number): void;
  repeat(): void;
  shuffle(): void;
  clear(): void;
  setRecordingField(
    recording: string,
    field: "title" | "artist",
    personal: string | null,
    inherited: string,
  ): void;
  setRecordingLiked?(recording: string, liked: boolean): void;
  reconnect?(): Promise<void>;
  retrySaved?(): Promise<void>;
}

export const UNSUPPORTED_FORMAT_MESSAGE =
  "This device can't play this audio format.";

export const MEDIA_ERR_SRC_NOT_SUPPORTED = 4;

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
  error?: { code: number } | null;
  seeking?: boolean;
  currentSrc?: string;
  play(): Promise<void>;
  pause(): void;
  load(): void;
  removeAttribute(name: string): void;
}

export type PlaybackState = PlaybackView & {
  index: number;
  position: number;
  duration: number;
};
