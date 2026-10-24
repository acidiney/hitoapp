import { log } from "../../../shared/log";
import { HttpError } from "../../../shared/http";
import {
  MAX_QUEUE_ENTRIES,
  type PlaybackQueueInput,
  type PlaybackStart,
  type PreparedSelection,
  type QueueSupply,
} from "../model";
import { QueueFeed } from "../QueueFeed";
import type { Action, Snapshot } from "./api";
import type { NativeSession, QueueSelection } from "./Session";

export class NativeCommands {
  private intention = 0;
  private queueEpoch = 0;
  private feed: QueueFeed;
  constructor(
    private session: () => NativeSession | null,
    private state: () => Snapshot | null,
    private report: (message: string) => void,
    pendingChanged: (pending: number) => void,
  ) {
    this.feed = new QueueFeed(
      () => pendingChanged(this.feed.pending),
      (message) => this.report(message),
    );
  }

  private replaceQueue() {
    this.feed.cancel();
    return ++this.queueEpoch;
  }

  playQueue = async (
    queue: PlaybackQueueInput[],
    index = 0,
    start?: PlaybackStart,
    more?: QueueSupply,
  ) => {
    this.intention++;
    await this.play(queue, index, start, more);
  };
  moveEntry = (entry: string, before: string | null) => {
    void this.run([{ kind: "move", entry, before }]);
  };
  continueAudiobook = async (audiobook: string, signal: AbortSignal) => {
    this.intention++;
    this.replaceQueue();
    const session = this.session();
    if (!session) {
      this.report("Connect this window to control playback.");
      return false;
    }
    log.debug("native.audiobook_continue_started");
    try {
      return await session.continueAudiobook(audiobook, signal);
    } catch (error) {
      if (
        this.session() === session &&
        !signal.aborted &&
        !(error instanceof DOMException && error.name === "AbortError")
      ) {
        this.report(
          error instanceof HttpError && error.status === 501
            ? "This saved playback speed cannot be restored by this player."
            : "Hito could not continue this audiobook. Try again.",
        );
        log.warn("native.audiobook_continue_failed");
      }
      return false;
    }
  };
  private async play(
    queue: PlaybackQueueInput[],
    index: number,
    start?: PlaybackStart,
    more?: QueueSupply,
  ) {
    const epoch = this.replaceQueue();
    const position = start?.kind === "saved" ? 0 : (start?.position ?? 0);
    if (
      !queue.length ||
      queue.length > 100000 ||
      !Number.isInteger(index) ||
      index < 0 ||
      index >= queue.length ||
      !Number.isFinite(position) ||
      position < 0 ||
      !Number.isSafeInteger(Math.round(position * 1000))
    ) {
      this.report("The playback selection is invalid or exceeds its limits.");
      return false;
    }
    const common = {
      start: index,
      from:
        start?.kind === "saved"
          ? { kind: "saved" as const }
          : {
              kind: "position" as const,
              position_ms: Math.round(position * 1000),
            },
    };
    const contextual = queue.some(
      (entry) => "recording" in entry && entry.chapter != null,
    );
    const selection: QueueSelection = contextual
      ? {
          ...common,
          entries: queue.map((entry) =>
            "recording" in entry
              ? {
                  recording_id: entry.recording.id,
                  ...(entry.chapter ? { chapter: { ...entry.chapter } } : {}),
                }
              : { recording_id: entry.id },
          ),
        }
      : {
          ...common,
          recordings: queue.map((entry) =>
            "recording" in entry ? entry.recording.id : entry.id,
          ),
        };
    const played = await this.run(selection);
    if (played && more && epoch === this.queueEpoch)
      this.feedQueue(more, epoch, queue.length);
    return played;
  }
  private feedQueue(more: QueueSupply, epoch: number, loaded: number) {
    let count = loaded;
    this.feed.start(more, async (batch) => {
      const session = this.session();
      if (!session || epoch !== this.queueEpoch)
        throw new DOMException("Superseded", "AbortError");
      const accepted = batch.slice(0, Math.max(0, MAX_QUEUE_ENTRIES - count));
      if (!accepted.length) return 0;
      count += accepted.length;
      await session.submit([
        {
          kind: "append",
          recordings: accepted.map((entry) =>
            "recording" in entry ? entry.recording.id : entry.id,
          ),
        },
      ]);
      return accepted.length;
    });
  }
  prepareSelection = (): PreparedSelection => {
    this.session()?.cancelAudiobook();
    const intention = ++this.intention;
    return async (target) => {
      if (intention !== this.intention) return false;
      this.intention++;
      return "queue" in target
        ? this.play(target.queue, target.index ?? 0, target.start, target.more)
        : this.play([target], 0);
    };
  };
  selectEntry = async (entry: string) => {
    this.intention++;
    await this.run([{ kind: "select", entry }, { kind: "resume" }]);
  };
  removeEntry = (entry: string) => {
    this.intention++;
    void this.run([{ kind: "remove", entry }]);
  };
  toggle = async () => {
    this.intention++;
    await this.setPlaying(!this.state()?.desired_playing);
  };
  setPlaying = async (playing: boolean) => {
    await this.run([{ kind: playing ? "resume" : "pause" }]);
  };
  retrySaved = async () => {
    this.intention++;
    await this.run([{ kind: "resume_saved" }]);
  };
  next = async () => {
    this.intention++;
    await this.run([{ kind: "next" }]);
  };
  seek = (seconds: number) => {
    const position = Math.round(Math.max(0, seconds) * 1000);
    if (Number.isSafeInteger(position))
      void this.run([{ kind: "seek", position_ms: position }]);
  };
  volume = (value: number) => {
    if (Number.isFinite(value))
      void this.run([
        {
          kind: "volume",
          level: Math.round(Math.min(1, Math.max(0, value)) * 1000),
        },
      ]);
  };
  clear = () => {
    this.intention++;
    this.replaceQueue();
    void this.run([{ kind: "clear" }]);
  };
  previous = async () => {
    this.intention++;
    await this.run([{ kind: "previous" }]);
  };
  speed = (value: number) => {
    if (Number.isFinite(value) && value >= 0.5 && value <= 3)
      void this.run([{ kind: "speed", rate: Math.round(value * 1000) }]);
  };
  repeat = () => {
    void this.run([{ kind: "repeat" }]);
  };
  shuffle = () => {
    void this.run([{ kind: "shuffle" }]);
  };
  invalidate() {
    this.session()?.cancelAudiobook();
    this.intention++;
    this.replaceQueue();
  }
  private async run(actions: Action[] | QueueSelection) {
    const session = this.session();
    if (!session) {
      this.report("Connect this window to control playback.");
      return false;
    }
    const action = Array.isArray(actions) ? actions[0]?.kind : "replace";
    log.debug("native.actions_started", {
      action,
      count: Array.isArray(actions)
        ? actions.length
        : "entries" in actions
          ? actions.entries.length
          : actions.recordings.length,
    });
    try {
      if (Array.isArray(actions)) await session.submit(actions);
      else await session.replace(actions);
      return this.session() === session;
    } catch (error) {
      if (
        this.session() === session &&
        !(error instanceof DOMException && error.name === "AbortError")
      ) {
        this.report("The playback command failed. Try again.");
        log.warn("native.actions_failed", { action });
      }
      return false;
    }
  }
}
