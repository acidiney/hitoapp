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
