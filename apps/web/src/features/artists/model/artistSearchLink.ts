export function artistSearchHref(recording: string) {
  return `/recordings/${encodeURIComponent(recording)}?review=artist`;
}

export function opensArtistSearch(search: string) {
  return new URLSearchParams(search).get("review") === "artist";
}
