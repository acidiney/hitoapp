import type { AudiobookChapterAnchor, Recording } from "../../shared/types";

export type Track = Pick<
  Recording,
  "id" | "title" | "artist" | "duration_ms" | "available"
> &
  Partial<Pick<Recording, "inherited_title" | "inherited_artist">>;

export type PlaybackStart = { kind: "saved" | "position"; position: number };
export type PlaybackQueueInput =
  | Track
  | { recording: Track; chapter?: AudiobookChapterAnchor };
export type PlaybackOccurrence = {
  id: string;
  recording: Track;
  chapter?: AudiobookChapterAnchor | null;
};
export type PlaybackSelection = {
  queue: PlaybackQueueInput[];
  index?: number;
  start?: PlaybackStart;
};
export type PreparedSelection = (
  selection: Track | PlaybackSelection,
) => Promise<boolean>;

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
  playQueue(
    queue: PlaybackQueueInput[],
    index?: number,
    start?: PlaybackStart,
  ): Promise<void>;
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
  reconnect?(): Promise<void>;
  retrySaved?(): Promise<void>;
}

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
  seeking?: boolean;
  currentSrc?: string;
  play(): Promise<void>;
  pause(): void;
  load(): void;
  removeAttribute(name: string): void;
}

export type PlaybackState = PlaybackView & {
  queue: Track[];
  index: number;
  position: number;
  duration: number;
};
