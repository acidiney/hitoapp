import { log } from "../../../shared/log";
import type { Player, PlaybackQueueInput, Track } from "../model";
import { recordingLookupApi } from "../api/recordingLookupApi";
import {
  STORED_QUEUE_LIMIT,
  STORED_QUEUE_LOOKBEHIND,
  browserStorage,
  clearStoredQueues,
  readStoredQueue,
  removeStoredQueue,
  serializeQueue,
  writeStoredQueue,
} from "./QueueStorage";

const POSITION_WRITE_INTERVAL_MS = 5000;

export type PersistedPlayer = Pick<
  Player,
  | "subscribe"
  | "subscribePosition"
  | "snapshot"
  | "queueSnapshot"
  | "positionSnapshot"
  | "queuePage"
> & {
  restoreQueue(
    queue: PlaybackQueueInput[],
    index: number,
    position: number,
  ): boolean;
};

type Options = {
  storage?: Storage | null;
  now?: () => number;
  lookup?: (recordings: string[], signal: AbortSignal) => Promise<Track[]>;
};

export class QueuePersistence {
  private readonly storage: Storage | null;
  private readonly now: () => number;
  private readonly lookup: NonNullable<Options["lookup"]>;
  private lastWrite = Number.NEGATIVE_INFINITY;
  private lastValue: string | null = null;
  private signature = "";
  private saving = false;
  private dirty = false;
  private stopped = false;

  constructor(
    private readonly player: PersistedPlayer,
    private readonly account: string,
    options: Options = {},
  ) {
    this.storage =
      options.storage === undefined ? browserStorage() : options.storage;
    this.now = options.now ?? Date.now;
    this.lookup =
      options.lookup ??
      ((recordings, signal) =>
        recordingLookupApi.read(this.account, recordings, signal));
  }

  async restore(signal: AbortSignal) {
    clearStoredQueues(this.storage, this.account);
    const stored = readStoredQueue(this.storage, this.account);
    if (!stored) {
      removeStoredQueue(this.storage, this.account);
      return;
    }
    const found = await this.lookup(
      [...new Set(stored.entries.map((entry) => entry.id))],
      signal,
    );
    if (signal.aborted) return;
    const byId = new Map(found.map((track) => [track.id, track]));
    const queue: PlaybackQueueInput[] = [];
    let index = -1;
    let before = 0;
    stored.entries.forEach((entry, at) => {
      const recording = byId.get(entry.id);
      if (!recording) {
        if (at < stored.index) before++;
        return;
      }
      if (at === stored.index) index = queue.length;
      queue.push(
        entry.chapter ? { recording, chapter: entry.chapter } : recording,
      );
    });
    const dropped = stored.entries.length - queue.length;
    const kept = index >= 0;
    if (!kept) index = Math.min(stored.index - before, queue.length - 1);
    if (index < 0) {
      removeStoredQueue(this.storage, this.account);
      return;
    }
    const current = queue[index];
    const duration =
      ("recording" in current ? current.recording : current).duration_ms / 1000;
    const position = kept ? stored.position : 0;
    const restored = this.player.restoreQueue(
      queue,
      index,
      duration > 0 ? Math.min(position, duration) : position,
    );
    if (dropped > 0) log.info("queue.restore_dropped", { count: dropped });
    log.info("queue.restore", { count: queue.length, enabled: restored });
  }

  start() {
    this.stopped = false;
    const stopStructure = this.player.subscribe(() => {
      const view = this.player.queueSnapshot();
      const signature = `${view.version}:${view.currentIndex}:${this.player.snapshot().playing}`;
      if (signature === this.signature) return;
      this.signature = signature;
      void this.save();
    });
    const stopPosition = this.player.subscribePosition(() => {
      if (this.now() - this.lastWrite >= POSITION_WRITE_INTERVAL_MS)
        void this.save();
    });
    const flush = () => void this.save();
    window.addEventListener("pagehide", flush);
    return () => {
      this.stopped = true;
      stopStructure();
      stopPosition();
      window.removeEventListener("pagehide", flush);
    };
  }

  private async save() {
    if (this.saving) {
      this.dirty = true;
      return;
    }
    this.saving = true;
    try {
      do {
        this.dirty = false;
        await this.write();
      } while (this.dirty && !this.stopped);
    } catch {
      log.warn("queue.persist_failed");
    } finally {
      this.saving = false;
    }
  }

  private async write() {
    const view = this.player.queueSnapshot();
    const index = view.currentIndex;
    if (view.total === 0 || index === null) {
      this.forget();
      return;
    }
    const first = Math.max(0, index - STORED_QUEUE_LOOKBEHIND);
    const page = await this.player.queuePage(first, STORED_QUEUE_LIMIT);
    if (this.stopped) return;
    const value = serializeQueue(
      page.entries.map((entry) =>
        entry.chapter
          ? { id: entry.recording.id, chapter: entry.chapter }
          : { id: entry.recording.id },
      ),
      index - first,
      this.player.positionSnapshot().position,
    );
    this.lastWrite = this.now();
    if (value !== this.lastValue) {
      this.lastValue = value;
      writeStoredQueue(this.storage, this.account, value);
    }
  }

  private forget() {
    this.lastValue = null;
    removeStoredQueue(this.storage, this.account);
  }
}
