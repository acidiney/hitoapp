import { regionName } from "../../../shared/regionName";
import type { Schema } from "../../../shared/schema";

type Candidate = Schema<"ArtistReviewCandidate">;

export function plural(count: number, one: string, many: string) {
  return `${count} ${count === 1 ? one : many}`;
}

export function candidateDetails(candidate: Candidate) {
  const parts = [
    candidate.disambiguation,
    candidate.kind,
    candidate.country ? regionName(candidate.country) : null,
  ];
  if (candidate.origin === "matched_on_other_tracks")
    parts.push(
      `Matched on ${plural(candidate.corroborated_titles, "other track", "other tracks")} of this credit`,
    );
  else if (candidate.corroborated_titles > 0)
    parts.push(
      `${plural(candidate.corroborated_titles, "title", "titles")} match your library`,
    );
  if (candidate.in_library) parts.push("already in your artists");
  if (candidate.origin === "chosen_artist")
    parts.unshift(
      `Same artist as ${candidate.name}`,
      "you already chose this artist for another credit",
    );
  return parts.filter(Boolean).join(" · ");
}

export function candidateLabel(candidate: Candidate) {
  return [candidate.name, candidate.disambiguation].filter(Boolean).join(", ");
}
