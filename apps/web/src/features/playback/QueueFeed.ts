import { log } from "../../shared/log";
import type { PlaybackQueueInput, QueueSupply } from "./model";

type Append = (
  batch: PlaybackQueueInput[],
  signal: AbortSignal,
) => Promise<number>;

export class QueueFeed {
  private run: { controller: AbortController; supply: QueueSupply } | null =
    null;

  constructor(
    private changed: () => void,
    private failed: (message: string) => void,
  ) {}

  get pending() {
    return this.run ? this.run.supply.remaining() : 0;
  }

  cancel() {
    if (!this.run) return;
    this.run.controller.abort();
    this.run = null;
    this.changed();
  }

  start(supply: QueueSupply | undefined, append: Append) {
    this.cancel();
    if (!supply || supply.remaining() <= 0) return;
    const run = { controller: new AbortController(), supply };
    this.run = run;
    this.changed();
    void this.pump(run, append);
  }

  private async pump(run: NonNullable<QueueFeed["run"]>, append: Append) {
    const { signal } = run.controller;
    const started = performance.now();
    let pages = 0;
    let message = "";
    try {
      while (!signal.aborted && run.supply.remaining() > 0) {
        const batch = await run.supply.next(signal);
        if (signal.aborted || !batch.length) break;
        const accepted = await append(batch, signal);
        if (signal.aborted) return;
        pages++;
        if (accepted < batch.length) {
          message = "Hito queued the first 100,000 tracks of this list.";
          break;
        }
        this.changed();
      }
    } catch {
      if (signal.aborted) return;
      message =
        "The rest of the list could not be loaded. Play it again to retry.";
    }
    if (this.run !== run) return;
    this.run = null;
    log.info("queue.feed_finished", {
      count: pages,
      reason: message ? "stopped" : "complete",
      duration_ms: Math.round(performance.now() - started),
    });
    this.changed();
    if (message) this.failed(message);
  }
}
