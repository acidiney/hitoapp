import type { Schema } from "../../../shared/schema";
import type { Player, QueueSupply } from "../../playback/model";

export const LOOKAHEAD = 3;

export type RowState =
  | { status: "idle" }
  | { status: "finding" }
  | { status: "found"; recording: string }
  | { status: "not_found" }
  | { status: "failed" };

export type Resolution =
  | { kind: "found"; recording: Schema<"Recording"> }
  | { kind: "not_found" }
  | { kind: "failed" }
  | { kind: "stop"; message: string; consent: boolean };

export const MAX_FAILURES_IN_A_ROW = 3;

export const SEARCH_FAILING_MESSAGE =
  "Your services are not answering right now. Hito stopped searching this album.";

export type CatalogRun = {
  occurrences: readonly string[];
  failures: number;
  ownedOnly: boolean;
  known(occurrence: string): Schema<"Recording"> | undefined;
  stopped(): boolean;
  find(occurrence: string, signal: AbortSignal): Promise<Resolution>;
  mark(occurrence: string, state: RowState): void;
  halt(stop: { message: string; consent: boolean }): void;
};

const stops: Partial<Record<Schema<"AlbumTrackAudio">["status"], string>> = {
  no_providers:
    "Connect an audio provider in Settings to play tracks that are not in your library.",
  disabled: "Metadata providers are disabled in your settings.",
  unsupported: "This provider does not support complete edition track lists.",
  needs_choice: "Open the album again to choose its edition.",
  search_off:
    "Playing a catalog album searches your connected services, and “Search connected services automatically” is off.",
};

export function resolution(outcome: Schema<"AlbumTrackAudio">): Resolution {
  switch (outcome.status) {
    case "saved":
      return { kind: "found", recording: outcome.recording };
    case "no_match":
    case "insufficient_evidence":
      return { kind: "not_found" };
    case "busy":
    case "changed":
    case "unavailable":
    case "cancelled":
      return { kind: "failed" };
    default:
      return {
        kind: "stop",
        message: stops[outcome.status]!,
        consent: outcome.status === "search_off",
      };
  }
}

export type Found = { index: number; recording: Schema<"Recording"> };

export async function nextFound(
  run: CatalogRun,
  from: number,
  signal: AbortSignal,
): Promise<Found | null> {
  for (let index = from; index < run.occurrences.length; index++) {
    signal.throwIfAborted();
    if (run.stopped()) return null;
    const occurrence = run.occurrences[index];
    if (run.ownedOnly) {
      const known = run.known(occurrence);
      if (!known) continue;
      run.mark(occurrence, { status: "found", recording: known.id });
      return { index, recording: known };
    }
    run.mark(occurrence, { status: "finding" });
    let result: Resolution;
    try {
      result = await run.find(occurrence, signal);
    } catch (error) {
      run.mark(occurrence, { status: "idle" });
      throw error;
    }
    if (result.kind === "found") {
      run.failures = 0;
      run.mark(occurrence, {
        status: "found",
        recording: result.recording.id,
      });
      return { index, recording: result.recording };
    }
    if (result.kind === "stop") {
      run.mark(occurrence, { status: "idle" });
      run.halt(result);
      if (!result.consent) return null;
      run.ownedOnly = true;
      continue;
    }
    run.mark(occurrence, { status: result.kind });
    run.failures = result.kind === "failed" ? run.failures + 1 : 0;
    if (run.failures >= MAX_FAILURES_IN_A_ROW) {
      run.halt({ message: SEARCH_FAILING_MESSAGE, consent: false });
      return null;
    }
  }
  return null;
}

function upcomingBelow(
  player: Pick<Player, "queueSnapshot" | "subscribe">,
  limit: number,
  signal: AbortSignal,
) {
  return new Promise<void>((resolve, reject) => {
    const wanted = () => {
      const queue = player.queueSnapshot();
      return (
        queue.currentIndex === null ||
        queue.total - 1 - queue.currentIndex < limit
      );
    };
    if (signal.aborted) return reject(signal.reason);
    if (wanted()) return resolve();
    const stop = () => {
      unsubscribe();
      signal.removeEventListener("abort", abort);
    };
    const abort = () => {
      stop();
      reject(signal.reason);
    };
    const unsubscribe = player.subscribe(() => {
      if (!wanted()) return;
      stop();
      resolve();
    });
    signal.addEventListener("abort", abort, { once: true });
  });
}

export function catalogSupply(
  run: CatalogRun,
  player: Pick<Player, "queueSnapshot" | "subscribe">,
  from: number,
): QueueSupply {
  let cursor = from;
  return {
    remaining: () => Math.max(0, run.occurrences.length - cursor),
    async next(signal) {
      await upcomingBelow(player, LOOKAHEAD, signal);
      if (run.stopped()) {
        cursor = run.occurrences.length;
        return [];
      }
      const found = await nextFound(run, cursor, signal);
      cursor = found ? found.index + 1 : run.occurrences.length;
      return found ? [found.recording] : [];
    },
  };
}
