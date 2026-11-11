import type { LyricCandidate } from "../hooks/useLyricSearch";

export type RecordingFacts = {
  title: string;
  artist: string;
  durationMs: number | null;
};

const durationToleranceMs = 2000;

export function normalizeLyricText(value: string): string {
  return value
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[([{][^)\]}]*[)\]}]/g, " ")
    .replace(/\b(feat|ft|featuring)\b\.?.*$/u, " ")
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim()
    .replace(/\s+/g, " ");
}

function artistMatches(candidate: string, wanted: string): boolean {
  const a = normalizeLyricText(candidate);
  const b = normalizeLyricText(wanted);
  return a !== "" && b !== "" && (a === b || a.includes(b) || b.includes(a));
}

export function confidentMatch(
  candidates: LyricCandidate[],
  facts: RecordingFacts,
): LyricCandidate | null {
  const title = normalizeLyricText(facts.title);
  if (title === "") return null;
  const confident = candidates.filter((item) => {
    if (normalizeLyricText(item.title) !== title) return false;
    if (!artistMatches(item.artist, facts.artist)) return false;
    if (facts.durationMs !== null && item.duration_ms != null)
      return (
        Math.abs(item.duration_ms - facts.durationMs) <= durationToleranceMs
      );
    return true;
  });
  if (confident.length === 0) return null;
  const distance = (item: LyricCandidate) =>
    facts.durationMs !== null && item.duration_ms != null
      ? Math.abs(item.duration_ms - facts.durationMs)
      : Number.MAX_SAFE_INTEGER;
  return confident.reduce((best, item) =>
    distance(item) < distance(best) ? item : best,
  );
}
