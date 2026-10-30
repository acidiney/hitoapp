import type { Schema } from "../../../shared/schema";

const matched: Record<
  Exclude<Schema<"ArtistAssociation">["evidence"], "user_confirmed">,
  string
> = {
  matched_file_tag_artist_id:
    "Matched automatically from an artist identifier in the file's tags.",
  matched_isrc: "Matched automatically from the recording's ISRC.",
  matched_provider_url:
    "Matched automatically from the provider's artist page.",
};

/** How an automatic match was made, or null for a review the account confirmed itself. */
export function associationEvidenceText(
  evidence: Schema<"ArtistAssociation">["evidence"],
): string | null {
  return evidence === "user_confirmed" ? null : matched[evidence];
}
