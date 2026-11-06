import { time } from "../../../shared/format";

const placeholderArtist = "unknown artist";
const sameLengthMs = 3000;

export function candidateArtist(artist: string): string | null {
  const name = artist.trim();
  return name && name.toLowerCase() !== placeholderArtist ? name : null;
}

export function candidateLength(durationMs: number | null | undefined) {
  return durationMs == null ? null : time(durationMs / 1000);
}

export function lengthComparison(
  savedMs: number | null | undefined,
  candidateMs: number | null | undefined,
): { text: string; close: boolean } | null {
  if (candidateMs == null)
    return { text: "The candidate does not give a length.", close: false };
  if (savedMs == null || savedMs <= 0) return null;
  const difference = candidateMs - savedMs;
  if (Math.abs(difference) <= sameLengthMs)
    return { text: "The lengths match.", close: true };
  const amount = time(Math.abs(difference) / 1000);
  const seconds = Math.round(Math.abs(difference) / 1000);
  const label = Math.abs(difference) < 60_000 ? `${seconds} seconds` : amount;
  return {
    text: `The candidate is ${label} ${difference > 0 ? "longer" : "shorter"} than your saved track.`,
    close: false,
  };
}
