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
  | { kind: "stop"; message: string };

export type CatalogRun = {
  occurrences: readonly string[];
  find(occurrence: string, signal: AbortSignal): Promise<Resolution>;
  mark(occurrence: string, state: RowState): void;
  halt(message: string): void;
};

const stops: Partial<Record<Schema<"AlbumTrackAudio">["status"], string>> = {
  no_providers:
    "Connect an audio provider in Settings to play tracks that are not in your library.",
  disabled: "Metadata providers are disabled in your settings.",
  unsupported: "This provider does not support complete edition track lists.",
  needs_choice: "Open the album again to choose its edition.",
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
      return { kind: "stop", message: stops[outcome.status]! };
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
    const occurrence = run.occurrences[index];
    run.mark(occurrence, { status: "finding" });
    let result: Resolution;
    try {
      result = await run.find(occurrence, signal);
    } catch (error) {
      run.mark(occurrence, { status: "idle" });
      throw error;
    }
    if (result.kind === "found") {
      run.mark(occurrence, {
        status: "found",
        recording: result.recording.id,
      });
      return { index, recording: result.recording };
    }
    if (result.kind === "stop") {
      run.mark(occurrence, { status: "idle" });
      run.halt(result.message);
      return null;
    }
    run.mark(occurrence, { status: result.kind });
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
      const found = await nextFound(run, cursor, signal);
      cursor = found ? found.index + 1 : run.occurrences.length;
      return found ? [found.recording] : [];
    },
  };
}
