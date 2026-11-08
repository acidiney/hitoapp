export type SearchPlacement = "library" | "global" | "none";

const GLOBAL_PAGES = new Set([
  "HomePage",
  "SearchPage",
  "DiscoverPage",
  "GenrePage",
  "SettingsPage",
  "QueuePage",
]);
const DETAIL_PAGES = new Set(["RecordingDetailsPage"]);

export function searchPlacement(
  component: string,
  phone: boolean,
): SearchPlacement {
  if (DETAIL_PAGES.has(component)) return "none";
  if (!GLOBAL_PAGES.has(component)) return "library";
  return phone ? "none" : "global";
}
