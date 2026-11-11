import { lyricsApi } from "./lyricsApi";
import { confidentMatch, type RecordingFacts } from "../model/lyricMatch";
import { lyricQuerySeed } from "../hooks/useLyricQuery";
import type { LyricIdentity } from "../model/lyricIdentity";

export type AutoIdentityResult =
  | { kind: "saved"; identity: LyricIdentity }
  | { kind: "none" }
  | { kind: "no_service" };

const finished = new Map<string, "none">();

function memoryKey(account: string, recording: string): string {
  return JSON.stringify([account, recording]);
}

export function autoLookupFinished(account: string, recording: string) {
  return finished.get(memoryKey(account, recording)) ?? null;
}

export function rememberAutoLookup(account: string, recording: string) {
  finished.set(memoryKey(account, recording), "none");
}

export function forgetAutoLookups() {
  finished.clear();
}

export async function findAutoIdentity(
  account: string,
  recording: string,
  facts: RecordingFacts,
  signal: AbortSignal,
): Promise<AutoIdentityResult> {
  const services = await lyricsApi.services(account, recording, signal);
  if (services.recording_id !== recording) throw new Error("changed");
  const service = services.items[0];
  if (!service) return { kind: "no_service" };
  const page = await lyricsApi.candidates(
    account,
    recording,
    {
      installation_id: service.installation_id,
      installation_revision: service.installation_revision,
      query: lyricQuerySeed(facts.title, facts.artist),
    },
    signal,
  );
  const match = confidentMatch(page.items, facts);
  if (!match) return { kind: "none" };
  const candidate = {
    installation_id: service.installation_id,
    installation_revision: service.installation_revision,
    provider_recording_id: match.id,
  };
  const review = await lyricsApi.review(account, recording, candidate, signal);
  if (review.candidate.recording.id !== match.id) throw new Error("changed");
  const identity = await lyricsApi.retain(
    account,
    recording,
    { candidate, review_stamp: review.review_stamp },
    signal,
  );
  return { kind: "saved", identity };
}
