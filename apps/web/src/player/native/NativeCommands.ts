import { log } from "../../log";
import type { PlaybackStart, PreparedSelection, Track } from "../model";
import type { Action, Snapshot } from "./api";
import type { NativeSession, QueueSelection } from "./Session";

export class NativeCommands {
  private intention = 0;
  constructor(
    private session: () => NativeSession | null,
    private state: () => Snapshot | null,
    private report: (message: string) => void,
  ) {}

  playQueue = async (queue: Track[], index = 0, start?: PlaybackStart) => {
    this.intention++;
    await this.play(queue, index, start);
  };
  private async play(queue: Track[], index: number, start?: PlaybackStart) {
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
    return this.run({
      recordings: queue.map((track) => track.id),
      start: index,
      from:
        start?.kind === "saved"
          ? { kind: "saved" }
          : { kind: "position", position_ms: Math.round(position * 1000) },
    });
  }
  prepareSelection = (): PreparedSelection => {
    const intention = ++this.intention;
    return async (target) => {
      if (intention !== this.intention) return false;
      this.intention++;
      return "queue" in target
        ? this.play(target.queue, target.index ?? 0, target.start)
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
    this.intention++;
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
