import type { Schema } from "../../../shared/schema";

function fold(text: string): string {
  return text
    .normalize("NFD")
    .replace(/\p{M}/gu, "")
    .trim()
    .toLocaleLowerCase();
}

/** Artists whose name is exactly the query come first; the server order is otherwise kept. */
export function rankArtists(
  query: string,
  artists: Schema<"Artist">[],
): Schema<"Artist">[] {
  const wanted = fold(query);
  const exact = artists.filter((artist) => fold(artist.name) === wanted);
  return [...exact, ...artists.filter((artist) => !exact.includes(artist))];
}

export type TopKind = "artist" | "track" | "release";

export function topResultKind(
  query: string,
  found: { tracks: number; releases: number; artists: Schema<"Artist">[] },
): TopKind | null {
  const wanted = fold(query);
  if (found.artists.some((artist) => fold(artist.name) === wanted))
    return "artist";
  if (found.tracks > 0) return "track";
  if (found.releases > 0) return "release";
  return found.artists.length > 0 ? "artist" : null;
}
