import type { Recording } from "../types";

export type Track = Pick<
  Recording,
  "id" | "title" | "artist" | "duration_ms" | "available"
> &
  Partial<Pick<Recording, "inherited_title" | "inherited_artist">>;

export type PlaybackStart = { kind: "saved" | "position"; position: number };

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
  entries: { id: string; recording: Track }[];
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
    queue: Track[],
    index?: number,
    start?: PlaybackStart,
  ): Promise<void>;
  prepareSelection(): (track: Track) => Promise<boolean>;
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
