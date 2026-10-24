import { log } from "../../shared/log";
import { HttpError } from "../../shared/http";
import type { PlaybackQueueInput, QueueSupply } from "./model";

type Append = (
  batch: PlaybackQueueInput[],
  signal: AbortSignal,
) => Promise<number>;

export class QueueStop extends Error {}

const BACKOFF_MS = [500, 1000, 2000];
const MAX_WAIT_MS = 10_000;
const RETRIED = [429, 502, 503, 504];

function pause(ms: number, signal: AbortSignal) {
  return new Promise<void>((resolve) => {
    const timer = setTimeout(resolve, ms);
    signal.addEventListener(
      "abort",
      () => {
        clearTimeout(timer);
        resolve();
      },
      { once: true },
    );
  });
}

async function page(supply: QueueSupply, signal: AbortSignal) {
  for (let attempt = 0; ; attempt++) {
    try {
      return await supply.next(signal);
    } catch (error) {
      const retriable =
        error instanceof HttpError
          ? RETRIED.includes(error.status)
          : error instanceof TypeError;
      if (signal.aborted || !retriable || attempt === BACKOFF_MS.length)
        throw error;
      const asked = error instanceof HttpError ? error.retryAfter : undefined;
      await pause(
        Math.min(
          MAX_WAIT_MS,
          asked !== undefined ? asked * 1000 : BACKOFF_MS[attempt],
        ),
        signal,
      );
      if (signal.aborted) throw error;
    }
  }
}

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
        const batch = await page(run.supply, signal);
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
    } catch (error) {
      if (signal.aborted) return;
      message =
        error instanceof QueueStop
          ? error.message
          : "The rest of the list could not be loaded. Play it again to retry.";
      log.warn("queue.feed_failed", {
        count: pages,
        reason: error instanceof Error ? error.name : "unknown",
        ...(error instanceof HttpError ? { status: error.status } : {}),
      });
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
