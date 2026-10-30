import type { Schema } from "../../../shared/schema";

export const VARIOUS_ARTISTS = "Various artists";

/**
 * The credit shown for an album: its own album artist, else the single artist
 * every loaded track shares, else "Various artists". Null when nothing is known.
 */
export function albumCreditLabel(
  albumArtist: string | null | undefined,
  trackArtists: readonly string[],
): string | null {
  const own = albumArtist?.trim();
  if (own) return own;
  const credits = new Set(
    trackArtists.map((artist) => artist.trim()).filter(Boolean),
  );
  if (credits.size === 0) return null;
  if (credits.size === 1) return [...credits][0];
  return VARIOUS_ARTISTS;
}
/**
 * The credit shown for a release: its own album artist, else the server's inference. The loaded
 * tracks decide only when the inference field is absent from the payload; a null inference means
 * the server found nothing to claim.
 */
export function releaseCreditLabel(
  release: Pick<Schema<"Release">, "artist" | "inferred_artist">,
  trackArtists: readonly string[] = [],
): string | null {
  const own = release.artist?.trim();
  if (own) return own;
  const inferred = release.inferred_artist;
  if (inferred === undefined) return albumCreditLabel(null, trackArtists);
  if (inferred === null) return null;
  return inferred.kind === "named" ? inferred.name : VARIOUS_ARTISTS;
}
