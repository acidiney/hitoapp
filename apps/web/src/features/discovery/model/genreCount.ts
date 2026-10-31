/** The server counts a genre only up to this many tracks. */
export const GENRE_COUNT_CAP = 1000;

export function genreCount(count: number): string {
  return count >= GENRE_COUNT_CAP
    ? `${GENRE_COUNT_CAP.toLocaleString("en-US")}+`
    : String(count);
}
