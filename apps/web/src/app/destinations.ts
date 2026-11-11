export type Destination =
  | "home"
  | "search"
  | "discover"
  | "library"
  | "plugins"
  | "settings"
  | null;

export type LibrarySection = { href: string; label: string; tab?: false };

export const LIBRARY_SECTIONS: LibrarySection[] = [
  { href: "/library", label: "Tracks" },
  { href: "/albums", label: "Albums" },
  { href: "/artists", label: "Artists" },
  { href: "/playlists", label: "Playlists" },
  { href: "/podcasts", label: "Podcasts" },
  { href: "/audiobooks", label: "Audiobooks" },
  { href: "/likes", label: "Liked tracks", tab: false },
  { href: "/history", label: "History" },
];

function within(path: string, prefix: string): boolean {
  return path === prefix || path.startsWith(`${prefix}/`);
}

export function currentSection(path: string): string | null {
  if (path === "/" || within(path, "/recordings")) return "/library";
  if (within(path, "/artist-groups")) return "/artists";
  const match = LIBRARY_SECTIONS.find((section) => within(path, section.href));
  return match?.href ?? null;
}

export function currentDestination(path: string, search = ""): Destination {
  if (within(path, "/home")) return "home";
  if (within(path, "/search")) return "search";
  if (within(path, "/discover") || within(path, "/genres")) return "discover";
  if (within(path, "/settings"))
    return new URLSearchParams(search).get("section") === "plugins"
      ? "plugins"
      : "settings";
  return currentSection(path) ? "library" : null;
}
