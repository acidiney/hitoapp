export type SearchPlacement = "library" | "global" | "none";

const GLOBAL_PAGES = new Set([
  "HomePage",
  "GenrePage",
  "SettingsPage",
  "QueuePage",
]);
const DETAIL_PAGES = new Set([
  "RecordingDetailsPage",
  "ArtistPage",
  "ArtistIdentityGroupPage",
  "CatalogAlbumPage",
  "AlbumPage",
  "PlaylistPage",
]);
const OWN_SEARCH_PAGES = new Set(["SearchPage", "DiscoverPage"]);

export function searchPlacement(
  component: string,
  phone: boolean,
): SearchPlacement {
  if (DETAIL_PAGES.has(component) || OWN_SEARCH_PAGES.has(component))
    return "none";
  if (!GLOBAL_PAGES.has(component)) return "library";
  return phone ? "none" : "global";
}
