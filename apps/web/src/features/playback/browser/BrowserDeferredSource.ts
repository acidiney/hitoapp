import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import type { SourcePreparer } from "../api/playbackSources";

const POLL_MS = 5_000;
const MAX_POLL_MS = 30_000;
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

/** Asks the provider to prepare one waiting source and reports whether it became ready. */
export async function prepareDeferredSource(
  preparer: SourcePreparer,
  recording: string,
  source: string,
  signal: AbortSignal,
): Promise<boolean> {
  const body = Object.freeze({ request_id: createUuid() });
  const started = performance.now();
  const fields = { recording_id: recording, request_id: body.request_id };
  log.info("playback.next_source_preparation_requested", fields);
  try {
    await preparer.start(recording, source, body, signal);
  } catch {
    if (!signal.aborted)
      log.warn("playback.next_source_preparation_rejected", fields);
    return false;
  }
  let failures = 0;
  while (!signal.aborted) {
    let delay = POLL_MS;
    try {
      const status = await preparer.status(recording, source, signal);
      if (signal.aborted) break;
      failures = 0;
      if (status?.phase === "ready") {
        log.info("playback.next_source_preparation_ready", {
          ...fields,
          duration_ms: Math.round(performance.now() - started),
        });
        return true;
      }
      if (status && STOPPED.has(status.phase)) {
        log.warn("playback.next_source_preparation_stopped", {
          ...fields,
          reason: status.phase,
        });
        return false;
      }
      if (status?.next_check_at != null)
        delay = Math.min(
          MAX_POLL_MS,
          Math.max(POLL_MS, status.next_check_at * 1000 - Date.now()),
        );
    } catch {
      if (signal.aborted || ++failures >= MAX_STATUS_FAILURES) {
        if (!signal.aborted)
          log.warn("playback.next_source_preparation_unchecked", fields);
        return false;
      }
    }
    await pause(delay, signal);
  }
  return false;
}
