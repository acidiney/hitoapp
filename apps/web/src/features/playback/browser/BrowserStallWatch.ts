export const STALL_DEADLINE_MS = 15_000;
export const SERVER_WAIT_MS = 120_000;

export type StallEngine = "playing" | "waiting" | "stopped";
export type StallStep = "rebuild" | "nextSource" | "giveUp";
export type ServerReach = "answered" | "unreachable";

export class BrowserStallWatch {
  private engine: StallEngine = "stopped";
  private due: number | null = null;
  private rebuiltOnce = false;
  private playingSince: number | null = null;
  private unreachableSince: number | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;

  constructor(private readonly onStep: (step: StallStep) => void) {}

  get buffering() {
    return this.engine === "waiting";
  }

  observe(next: StallEngine) {
    const now = performance.now();
    if (next === "playing") {
      if (this.engine !== "playing") this.playingSince = now;
      this.due = null;
      this.unreachableSince = null;
    } else if (next === "waiting") {
      if (this.engine === "waiting") return;
      if (
        this.playingSince !== null &&
        now - this.playingSince >= STALL_DEADLINE_MS
      )
        this.rebuiltOnce = false;
      this.playingSince = null;
      this.due = now + STALL_DEADLINE_MS;
    } else {
      this.playingSince = null;
      this.due = null;
      this.unreachableSince = null;
    }
    this.engine = next;
    this.schedule();
  }

  rebuilt(reach: ServerReach) {
    const now = performance.now();
    if (reach === "answered") this.unreachableSince = null;
    else {
      this.rebuiltOnce = false;
      this.unreachableSince ??= now;
    }
    this.due = now + STALL_DEADLINE_MS;
    this.schedule();
  }

  reset() {
    this.engine = "stopped";
    this.due = null;
    this.rebuiltOnce = false;
    this.playingSince = null;
    this.unreachableSince = null;
    this.schedule();
  }

  private escalate(): StallStep {
    const now = performance.now();
    this.due = now + STALL_DEADLINE_MS;
    this.schedule();
    if (
      this.unreachableSince !== null &&
      now - this.unreachableSince >= SERVER_WAIT_MS
    )
      return "giveUp";
    if (this.rebuiltOnce) return "nextSource";
    this.rebuiltOnce = true;
    return "rebuild";
  }

  private schedule() {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
    if (this.due === null) return;
    this.timer = setTimeout(
      () => {
        this.timer = null;
        this.onStep(this.escalate());
      },
      Math.max(0, Math.ceil(this.due - performance.now())),
    );
  }
}
