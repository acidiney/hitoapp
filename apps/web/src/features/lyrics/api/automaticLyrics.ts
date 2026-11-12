import { lyricsApi } from "./lyricsApi";
import { automaticOutcome, type AutomaticOutcome } from "../model/lyricContent";

type Settled = Exclude<AutomaticOutcome["kind"], "lyrics">;

const rememberable: Settled[] = [
  "not_found",
  "insufficient_metadata",
  "ambiguous",
];

const finished = new Map<string, Settled>();

function memoryKey(account: string, recording: string): string {
  return JSON.stringify([account, recording]);
}

export function rememberedAutomatic(
  account: string,
  recording: string,
): AutomaticOutcome | null {
  const kind = finished.get(memoryKey(account, recording));
  return kind ? { kind } : null;
}

export function forgetAutomaticLookups() {
  finished.clear();
}

export async function lookupAutomatic(
  account: string,
  recording: string,
  signal: AbortSignal,
): Promise<AutomaticOutcome | null> {
  const lookup = await lyricsApi.automatic(account, recording, signal);
  if (signal.aborted) return { kind: "cancelled" };
  const outcome = automaticOutcome(lookup, recording);
  if (outcome && rememberable.includes(outcome.kind as Settled))
    finished.set(memoryKey(account, recording), outcome.kind as Settled);
  return outcome;
}
