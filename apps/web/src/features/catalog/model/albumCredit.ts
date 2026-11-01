import type { Schema } from "../../../shared/schema";

export const VARIOUS_ARTISTS = "Various artists";

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
