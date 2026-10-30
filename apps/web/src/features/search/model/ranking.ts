export function fold(text: string): string {
  return text
    .normalize("NFD")
    .replace(/\p{M}/gu, "")
    .trim()
    .toLocaleLowerCase();
}

export type TopKind = "artist" | "track" | "release";

export function topResultKind(
  query: string,
  found: { tracks: number; releases: number; artistNames: string[] },
): TopKind | null {
  const wanted = fold(query);
  if (found.artistNames.some((name) => fold(name) === wanted)) return "artist";
  if (found.tracks > 0) return "track";
  if (found.releases > 0) return "release";
  return found.artistNames.length > 0 ? "artist" : null;
}
