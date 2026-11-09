function count(value: number, one: string, many: string) {
  return `${value} ${value === 1 ? one : many}`;
}

export function artistCounts(tracks: number, albums: number, featured = 0) {
  const parts = [
    tracks > 0 ? count(tracks, "track", "tracks") : "",
    albums > 0 ? count(albums, "album", "albums") : "",
    featured > 0 ? count(featured, "appearance", "appearances") : "",
  ].filter(Boolean);
  return parts.length ? parts.join(" · ") : "0 tracks";
}
