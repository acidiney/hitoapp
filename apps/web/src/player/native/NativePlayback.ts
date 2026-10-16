import { log } from "../../log";
import { HttpError } from "../../http";
import type {
  PlaybackPosition,
  PlaybackView,
  QueuePage,
  QueueView,
  Player,
} from "../model";
import type { Position, Snapshot } from "./api";
import { NativeSession, type Failure } from "./Session";
import { NativeCommands } from "./NativeCommands";

function emptyView(): PlaybackView {
  return {
    current: null,
    playing: false,
    loading: false,
    error: "",
    repeat: "off",
    shuffle: false,
    speed: 1,
    volume: 1,
    connection: "connecting",
  };
}

function playbackError(value: Snapshot | null) {
  return value?.phase === "suspended"
    ? "Playback paused while Hito checks access. Your queue is unchanged."
    : value?.persistence_error
      ? "Listening progress could not be saved. Hito will retry."
      : value?.error
        ? "Audio could not be played. Try selecting the track again."
        : value?.last_failure
          ? "Audio was unavailable for a queued track."
          : "";
}

export class NativePlayback implements Player {
  readonly mode = "native";
  readonly supports = {
    previous: false,
    shuffle: false,
    repeat: false,
    speed: false,
  };
  private view = emptyView();
  private position: PlaybackPosition = { position: 0, duration: 0 };
  private queueView: QueueView = {
    version: 0,
    total: 0,
    currentIndex: null,
    currentEntry: null,
  };
  private listeners = new Set<() => void>();
  private positionListeners = new Set<() => void>();
  private session: NativeSession | null = null;
  private wire: Snapshot | null = null;
  private latestPosition: Position | null = null;
  private metadata: AbortController | null = null;
  private intents = new NativeCommands(
    () => this.session,
    () => this.wire,
    (error) => this.update({ error }),
  );
  playQueue = this.intents.playQueue;
  prepareSelection = this.intents.prepareSelection;
  selectEntry = this.intents.selectEntry;
  removeEntry = this.intents.removeEntry;
  toggle = this.intents.toggle;
  next = this.intents.next;
  previous = this.intents.previous;
  seek = this.intents.seek;
  volume = this.intents.volume;
  clear = this.intents.clear;
  speed = this.intents.speed;
  shuffle = this.intents.shuffle;
  repeat = this.intents.repeat;

  constructor(private account: string) {}
  snapshot = () => this.view;
  positionSnapshot = () => this.position;
  queueSnapshot = () => this.queueView;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  subscribePosition = (listener: () => void) => {
    this.positionListeners.add(listener);
    return () => {
      this.positionListeners.delete(listener);
    };
  };

  reconnect = async () => {
    this.close();
    this.update({ ...emptyView() });
    const session = new NativeSession(this.account, {
      state: (value) => {
        if (this.session === session) this.observeState(value);
      },
      position: (value) => {
        if (this.session === session) this.observePosition(value);
      },
      failure: (kind) => {
        if (this.session === session) this.failed(kind);
      },
      connection: (ready) => {
        if (this.session !== session) return;
        this.update({
          connection: ready ? "ready" : "disconnected",
          ...(ready &&
          this.view.error === "Playback connection interrupted. Reconnecting…"
            ? { error: playbackError(this.wire) }
            : {}),
        });
      },
    });
    this.session = session;
    try {
      await session.open();
    } catch {
      if (this.session === session) this.failed("ownership");
    }
  };

  async queuePage(
    offset: number,
    limit: number,
    signal?: AbortSignal,
  ): Promise<QueuePage> {
    const session = this.session;
    const wire = this.wire;
    const view = this.queueView;
    if (!session || !wire)
      throw new Error("Connect the player to view its queue.");
    const page = await session.queue(wire.revision, offset, limit, signal);
    signal?.throwIfAborted();
    if (
      this.session !== session ||
      this.queueView !== view ||
      page.revision !== wire.revision
    )
      throw new DOMException("Superseded", "AbortError");
    return {
      version: view.version,
      total: page.total,
      offset: page.offset,
      entries: page.entries,
    };
  }

  private observeState(value: Snapshot) {
    const old = this.wire;
    if (
      old &&
      (value.revision < old.revision || value.selection < old.selection)
    )
      return;
    const changed =
      !old ||
      old.revision !== value.revision ||
      old.selection !== value.selection ||
      old.current?.id !== value.current?.id ||
      old.error !== value.error ||
      old.last_failure?.entry.id !== value.last_failure?.entry.id;
    this.wire = value;
    if (changed) {
      this.queueView = {
        version: this.queueView.version + 1,
        total: value.queue_len,
        currentIndex: value.current_index ?? null,
        currentEntry: value.current?.id ?? null,
      };
      if (old?.selection !== value.selection)
        this.updatePosition({ position: 0, duration: 0 });
    }
    this.update({
      current:
        this.view.current?.id === value.current?.recording_id
          ? this.view.current
          : null,
      playing: value.playing,
      desiredPlaying: value.desired_playing,
      loading:
        value.phase === "resolving" ||
        value.phase === "opening" ||
        value.phase === "releasing" ||
        value.seeking,
      volume: value.volume / 1000,
      error: playbackError(value),
    });
    if (changed) this.loadCurrent();
    if (this.latestPosition) this.observePosition(this.latestPosition);
  }

  private loadCurrent() {
    this.metadata?.abort();
    const controller = new AbortController();
    this.metadata = controller;
    const wire = this.wire;
    const queue = this.queueView;
    if (
      !wire?.current ||
      wire.current_index === null ||
      wire.current_index === undefined
    )
      return;
    void this.queuePage(wire.current_index, 1, controller.signal)
      .then((page) => {
        if (controller.signal.aborted || queue !== this.queueView) return;
        const track = page.entries[0]?.recording;
        if (!track || track.id !== wire.current?.recording_id)
          throw new Error("Current entry is unavailable.");
        this.update({ current: track });
        if (!this.position.duration)
          this.updatePosition({
            ...this.position,
            duration: track.duration_ms / 1000,
          });
      })
      .catch((error: unknown) => {
        if (controller.signal.aborted || queue !== this.queueView) return;
        const denied =
          error instanceof HttpError && [401, 403, 404].includes(error.status);
        this.update({
          ...(denied ? { current: null } : {}),
          error:
            playbackError(this.wire) ||
            (denied
              ? "The selected track is unavailable to this account."
              : "Track details could not be loaded. Playback controls are still available."),
        });
      });
  }

  private observePosition(value: Position) {
    const wire = this.wire;
    if (wire && value.selection < wire.selection) return;
    this.latestPosition = value;
    if (
      !wire ||
      value.selection !== wire.selection ||
      value.entry !== wire.current?.id
    )
      return;
    this.updatePosition({
      position: value.position_ms / 1000,
      duration:
        (value.duration_ms ?? this.view.current?.duration_ms ?? 0) / 1000,
    });
  }

  private failed(kind: Failure) {
    log.warn("native.connection_failed", {
      account_id: this.account,
      reason: kind,
    });
    if (kind === "unavailable") {
      this.update({
        connection: "disconnected",
        error: "Playback connection interrupted. Reconnecting…",
      });
      return;
    }
    this.close();
    this.update({
      connection: "disconnected",
      error:
        kind === "ownership"
          ? "Playback ownership changed or your session ended. Reconnect this window to continue."
          : "Playback returned an invalid response. Reconnect to continue.",
    });
  }
  private update(patch: Partial<PlaybackView>) {
    this.view = { ...this.view, ...patch };
    this.listeners.forEach((listener) => listener());
  }
  setRecordingField() {
    this.queueView = { ...this.queueView, version: this.queueView.version + 1 };
    this.loadCurrent();
    this.listeners.forEach((listener) => listener());
  }
  private updatePosition(value: PlaybackPosition) {
    this.position = value;
    this.positionListeners.forEach((listener) => listener());
  }
  close() {
    this.intents.invalidate();
    this.session?.close();
    this.session = null;
    this.metadata?.abort();
    this.metadata = null;
    this.wire = null;
    this.latestPosition = null;
    this.queueView = {
      version: this.queueView.version + 1,
      total: 0,
      currentIndex: null,
      currentEntry: null,
    };
    this.update({ ...emptyView(), connection: "disconnected" });
    this.updatePosition({ position: 0, duration: 0 });
  }
}
