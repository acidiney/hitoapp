import type { Schema } from "../../../shared/schema";

type Association = Schema<"ArtistAssociation">;

const matched: Record<
  Exclude<
    Association["evidence"],
    "user_confirmed" | "matched_credit_evidence" | "matched_credit_coverage"
  >,
  string
> = {
  matched_file_tag_artist_id:
    "Matched automatically from an artist identifier in the file's tags.",
  matched_isrc: "Matched automatically from the recording's ISRC.",
  matched_provider_url:
    "Matched automatically from the provider's artist page.",
  matched_name_discography:
    "Matched automatically from the artist's name, corroborated by its recordings.",
  user_confirmed_candidate:
    "You confirmed this artist from the candidates Hito found.",
};

export function providerName(
  provider: Schema<"ArtistAssociation">["provider"],
): string {
  return provider.kind === "builtin_musicbrainz"
    ? "MusicBrainz"
    : "Metadata plugin";
}

const creditSource: Record<Schema<"CreditEvidence">["kind"], string> = {
  file_tag_artist_id: "artist identifier in file tags",
  isrc: "ISRC",
  provider_url: "provider artist page",
  name_discography: "name corroborated by recordings",
};

function creditEvidenceText(evidence: Schema<"CreditEvidence"> | null) {
  if (!evidence)
    return "Matched automatically from other tracks of this credit.";
  const tracks =
    evidence.tracks === 1 ? "1 track's" : `${evidence.tracks} tracks'`;
  return `Matched automatically from ${tracks} ${creditSource[evidence.kind]} in this credit.`;
}

function coverageEvidenceText(evidence: Schema<"CoverageEvidence"> | null) {
  if (!evidence)
    return "Matched automatically from other tracks of this credit.";
  return `Matched from ${evidence.matched_tracks} of ${evidence.probed_tracks} tracks of this artist.`;
}

/** How an automatic match was made, or null for a review the account confirmed itself. */
export function associationEvidenceText(
  association: Pick<
    Association,
    "evidence" | "credit_evidence" | "coverage_evidence"
  >,
): string | null {
  const { evidence } = association;
  if (evidence === "user_confirmed") return null;
  if (evidence === "matched_credit_evidence")
    return creditEvidenceText(association.credit_evidence);
  if (evidence === "matched_credit_coverage")
    return coverageEvidenceText(association.coverage_evidence);
  return matched[evidence];
}

const tracksBy: Record<Association["evidence"], string> = {
  matched_file_tag_artist_id: "an artist identifier in the file's tags",
  matched_isrc: "its ISRC",
  matched_provider_url: "the provider's artist page",
  matched_name_discography: "the artist's name, corroborated by its recordings",
  matched_credit_evidence: "other tracks of this credit",
  matched_credit_coverage:
    "the share of this artist's tracks the credit covers",
  user_confirmed_candidate: "a candidate you confirmed",
  user_confirmed: "your own choice",
};

/** How a credit's tracks reached their artist, or null when the account chose them all itself. */
export function chosenEvidenceText(
  evidence: Schema<"ArtistMatchChosen">["evidence"],
): string | null {
  if (evidence.every((how) => how.evidence === "user_confirmed")) return null;
  const parts = evidence.map(
    (how) =>
      `${how.tracks === 1 ? "1 track" : `${how.tracks} tracks`}: ${tracksBy[how.evidence]}`,
  );
  return `Matched from: ${parts.join("; ")}`;
}
