function count(value: number, one: string, many: string) {
  return `${value} ${value === 1 ? one : many}`;
}

export function artistCounts(tracks: number, albums: number) {
  const parts = [
    tracks > 0 ? count(tracks, "track", "tracks") : "",
    albums > 0 ? count(albums, "album", "albums") : "",
  ].filter(Boolean);
  return parts.length ? parts.join(" · ") : "0 tracks";
}
