function count(value: number, one: string, many: string) {
  return `${value} ${value === 1 ? one : many}`;
}

export function artistCounts(
  tracks: number,
  albums: number,
  featured = 0,
  scope: "plain" | "credit" | "chosen" = "plain",
) {
  const parts = [
    tracks > 0
      ? `${count(tracks, "track", "tracks")}${scope === "chosen" ? " in your library" : ""}`
      : "",
    albums > 0
      ? `${count(albums, "album", "albums")}${scope === "credit" ? " in your library" : ""}`
      : "",
    featured > 0 ? count(featured, "appearance", "appearances") : "",
  ].filter(Boolean);
  return parts.length ? parts.join(" · ") : "0 tracks";
}
