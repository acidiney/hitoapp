import type { Schema } from "../../../shared/schema";

type Candidate = Schema<"ArtistReviewCandidate">;

export function plural(count: number, one: string, many: string) {
  return `${count} ${count === 1 ? one : many}`;
}

/** What tells namesakes apart: the MusicBrainz note, type and country, then what the library shares. */
export function candidateDetails(candidate: Candidate) {
  const parts = [candidate.disambiguation, candidate.kind, candidate.country];
  if (candidate.corroborated_titles > 0)
    parts.push(
      `${plural(candidate.corroborated_titles, "title", "titles")} match your library`,
    );
  if (candidate.in_library) parts.push("already in your artists");
  return parts.filter(Boolean).join(" · ");
}

export function candidateLabel(candidate: Candidate) {
  return [candidate.name, candidate.disambiguation].filter(Boolean).join(", ");
}
