import type { PlaybackState, QueueView } from "../model";
import type { BrowserQueue } from "./BrowserQueue";

export class PlaybackStore {
  state: PlaybackState = {
    current: null,
    index: -1,
    playing: false,
    loading: false,
    position: 0,
    duration: 0,
    error: "",
    repeat: "off",
    shuffle: false,
    speed: 1,
    effectiveSpeed: 1,
    speedRange: null,
    volume: 1,
    connection: "ready",
  };
  queueView: QueueView = {
    version: 0,
    total: 0,
    currentIndex: null,
    currentEntry: null,
    pending: 0,
  };
  private position = { position: 0, duration: 0 };
  private listeners = new Set<() => void>();
  private positionListeners = new Set<() => void>();

  snapshot = () => this.state;
  queueSnapshot = () => this.queueView;
  positionSnapshot = () => this.position;
  subscribePosition = (listener: () => void) => {
    this.positionListeners.add(listener);
    return () => {
      this.positionListeners.delete(listener);
    };
  };
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  setPending(pending: number) {
    if (pending === this.queueView.pending) return;
    this.queueView = {
      ...this.queueView,
      version: this.queueView.version + 1,
      pending,
    };
    this.listeners.forEach((listener) => listener());
  }

  update(
    patch: Partial<PlaybackState> & { queueChanged?: true },
    queue: BrowserQueue,
  ) {
    const { queueChanged, ...changes } = patch;
    const { position, duration, ...structure } = changes;
    const moved =
      (position !== undefined && position !== this.position.position) ||
      (duration !== undefined && duration !== this.position.duration);
    this.state = { ...this.state, ...changes };
    if (queueChanged || patch.index !== undefined) {
      const current = queue.at(this.state.index)?.id;
      this.queueView = {
        version: this.queueView.version + 1,
        total: queue.length,
        currentIndex: current ? this.state.index : null,
        currentEntry: current ?? null,
        pending: this.queueView.pending,
      };
    }
    if (moved) {
      this.position = {
        position: this.state.position,
        duration: this.state.duration,
      };
      this.positionListeners.forEach((listener) => listener());
    }
    if (queueChanged || Object.keys(structure).length)
      this.listeners.forEach((listener) => listener());
  }
}
