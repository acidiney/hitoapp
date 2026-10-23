import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import {
  MIN_CHECK_MS,
  msUntilCheck,
} from "../../sources/api/preparationPolicy";
import type { SourcePreparer } from "../api/playbackSources";

const MAX_STATUS_FAILURES = 3;
const STOPPED = new Set([
  "failed",
  "blocked",
  "cancelled",
  "submission_unknown",
]);

function pause(ms: number, signal: AbortSignal) {
  return new Promise<void>((resolve) => {
    const done = () => {
      clearTimeout(timer);
      signal.removeEventListener("abort", done);
      resolve();
    };
    const timer = setTimeout(done, ms);
    signal.addEventListener("abort", done);
  });
}

export type Submission = "open" | "stopped";

/** Asks the provider to prepare one waiting source and reports whether it became ready. */
export async function prepareDeferredSource(
  preparer: SourcePreparer,
  recording: string,
  source: string,
  signal: AbortSignal,
  budget: { remainingMs: () => number },
  previous: Submission | undefined,
  record: (state: Submission) => void,
): Promise<boolean> {
  if (previous === "stopped") return false;
  const body = Object.freeze({ request_id: createUuid() });
  const started = performance.now();
  const fields = { recording_id: recording, request_id: body.request_id };
  if (previous === undefined) {
    record("open");
    log.info("playback.next_source_preparation_requested", fields);
    try {
      await preparer.start(recording, source, body, signal);
    } catch {
      if (!signal.aborted) {
        record("stopped");
        log.warn("playback.next_source_preparation_rejected", fields);
      }
      return false;
    }
  } else log.debug("playback.next_source_preparation_resumed", fields);
  let failures = 0;
  while (!signal.aborted) {
    let delay = MIN_CHECK_MS;
    try {
      const status = await preparer.status(recording, source, signal);
      if (signal.aborted) break;
      failures = 0;
      if (status?.phase === "ready") {
        record("stopped");
        log.info("playback.next_source_preparation_ready", {
          ...fields,
          duration_ms: Math.round(performance.now() - started),
        });
        return true;
      }
      if (status && STOPPED.has(status.phase)) {
        record("stopped");
        log.warn("playback.next_source_preparation_stopped", {
          ...fields,
          reason: status.phase,
        });
        return false;
      }
      delay = Math.max(delay, msUntilCheck(status));
    } catch {
      if (signal.aborted || ++failures >= MAX_STATUS_FAILURES) {
        if (!signal.aborted)
          log.warn("playback.next_source_preparation_unchecked", fields);
        return false;
      }
    }
    if (delay >= budget.remainingMs()) {
      log.debug("playback.next_source_preparation_deferred", fields);
      return false;
    }
    await pause(delay, signal);
  }
  return false;
}
