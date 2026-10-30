import type { Schema } from "../../../shared/schema";
import { prepareChange, submitChange } from "../../../shared/sync";
import { log } from "../../../shared/log";
import { HttpError } from "../../../shared/http";
import { createUuid } from "../../../shared/uuid";

import type { BrowserCheckpoint } from "./BrowserObservation";

type Scope = { signal: AbortSignal; active(): boolean };
type Pending = { sequence: number; batch: Schema<"SyncBatch"> };
type Lane = {
  scope: Scope;
  issued: number;
  settled: number;
  pending: Pending[];
  running: boolean;
  blocked: boolean;
  waiters: {
    sequence: number;
    resolve(): void;
    reject(error: Error): void;
  }[];
};

const persistenceError = () =>
  new Error("Listening progress could not be saved. Hito will retry.");

export class BrowserProgress {
  private lane: Lane | null = null;

  constructor(
    private account: string,
    private capture: () => Scope,
    private failed: (message: string) => void,
    private available: () => void,
  ) {}

  admit(sample: BrowserCheckpoint): boolean {
    const scope = this.capture();
    if (!scope.active()) return false;
    if (this.lane?.scope.signal !== scope.signal) {
      const lane: Lane = {
        scope,
        issued: 0,
        settled: 0,
        pending: [],
        running: false,
        blocked: false,
        waiters: [],
      };
      this.lane = lane;
      scope.signal.addEventListener(
        "abort",
        () => {
          lane.pending = [];
          this.reject(lane, new DOMException("Retired", "AbortError"));
          log.debug("playback.progress_retired", { account_id: this.account });
        },
        { once: true },
      );
    }
    const lane = this.lane!;
    if (lane.pending.length >= 256) {
      this.retry();
      this.failed(
        "Listening progress is waiting to be saved. Playback paused.",
      );
      log.warn("playback.progress_capacity", {
        account_id: this.account,
        count: lane.pending.length,
      });
      return false;
    }
    const batch = prepareChange({
      kind: "progress_set",
      recording_id: sample.recording.id,
      position_ms: Math.floor(sample.position * 1000),
      speed: sample.speed,
    });
    if (sample.chapter)
      batch.operations.push({
        id: createUuid(),
        action: {
          kind: "audiobook_chapter_set",
          chapter: Object.freeze({ ...sample.chapter }),
        },
      });
    for (const operation of batch.operations) {
      Object.freeze(operation.action);
      Object.freeze(operation);
    }
    Object.freeze(batch.operations);
    Object.freeze(batch);
    lane.pending.push({ sequence: ++lane.issued, batch });
    log.debug("playback.progress_admitted", {
      account_id: this.account,
      operation_id: batch.operations[0].id,
      count: lane.pending.length,
    });
    this.retry();
    return true;
  }
  flush(signal: AbortSignal): Promise<void> {
    signal.throwIfAborted();
    const lane = this.lane;
    if (!lane) return Promise.resolve();
    const sequence = lane.issued;
    if (!lane.scope.active())
      return Promise.reject(new DOMException("Retired", "AbortError"));
    if (lane.settled >= sequence) return Promise.resolve();
    if (lane.blocked) return Promise.reject(persistenceError());
    if (lane.waiters.length >= 256) return Promise.reject(persistenceError());
    return new Promise((resolve, reject) => {
      const waiter = {
        sequence,
        resolve: () => {
          signal.removeEventListener("abort", cancel);
          resolve();
        },
        reject: (error: Error) => {
          signal.removeEventListener("abort", cancel);
          reject(error);
        },
      };
      const cancel = () => {
        lane.waiters = lane.waiters.filter((pending) => pending !== waiter);
        waiter.reject(new DOMException("Retired", "AbortError"));
      };
      signal.addEventListener("abort", cancel, { once: true });
      lane.waiters.push(waiter);
    });
  }
  retry() {
    const lane = this.lane;
    if (!lane || lane.running || !lane.scope.active()) return;
    lane.blocked = false;
    lane.running = true;
    void this.drain(lane).finally(() => {
      lane.running = false;
    });
  }
  private async drain(lane: Lane) {
    while (lane.pending.length && lane.scope.active()) {
      const head = lane.pending[0];
      const started = performance.now();
      let accepted = false;
      let rejected: number | null = null;
      for (let attempt = 0; attempt < 2 && lane.scope.active(); attempt++) {
        const controller = new AbortController();
        const cancel = () => controller.abort();
        lane.scope.signal.addEventListener("abort", cancel, { once: true });
        const deadline = setTimeout(() => {
          cancel();
          log.debug("playback.progress_deadline", {
            account_id: this.account,
            operation_id: head.batch.operations[0].id,
            count: attempt + 1,
            duration_ms: Math.round(performance.now() - started),
          });
        }, 10_000);
        try {
          await submitChange(this.account, head.batch, controller.signal);
          if (!lane.scope.active()) return;
          accepted = true;
          break;
        } catch (error) {
          if (!lane.scope.active()) return;
          if (
            error instanceof HttpError &&
            [401, 403, 404].includes(error.status)
          ) {
            rejected = error.status;
            break;
          }
          log.debug("playback.progress_retry", {
            account_id: this.account,
            operation_id: head.batch.operations[0].id,
            count: attempt + 1,
          });
        } finally {
          clearTimeout(deadline);
          lane.scope.signal.removeEventListener("abort", cancel);
        }
      }
      if (!accepted && rejected === null) {
        lane.blocked = true;
        this.reject(lane, persistenceError());
        this.failed(persistenceError().message);
        log.warn("playback.progress_blocked", {
          account_id: this.account,
          operation_id: head.batch.operations[0].id,
          count: lane.pending.length,
        });
        return;
      }
      lane.pending.shift();
      lane.settled = head.sequence;
      const ready = lane.waiters.filter(
        ({ sequence }) => sequence <= lane.settled,
      );
      lane.waiters = lane.waiters.filter(
        ({ sequence }) => sequence > lane.settled,
      );
      ready.forEach(({ resolve }) => resolve());
      const result = {
        account_id: this.account,
        operation_id: head.batch.operations[0].id,
        duration_ms: Math.round(performance.now() - started),
      };
      if (rejected === null) log.debug("playback.progress_accepted", result);
      else {
        this.failed("Listening progress could not be saved for this item.");
        log.warn("playback.progress_rejected", { ...result, status: rejected });
      }
      this.available();
    }
  }
  private reject(lane: Lane, error: Error) {
    const waiters = lane.waiters;
    lane.waiters = [];
    waiters.forEach(({ reject }) => reject(error));
  }
}
