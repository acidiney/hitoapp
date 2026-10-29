export type Destination = "home" | "library" | "settings" | null;

export type LibrarySection = { href: string; label: string };

export const LIBRARY_SECTIONS: LibrarySection[] = [
  { href: "/library", label: "Library" },
  { href: "/albums", label: "Albums" },
  { href: "/artists", label: "Artists" },
  { href: "/playlists", label: "Playlists" },
  { href: "/podcasts", label: "Podcasts" },
  { href: "/audiobooks", label: "Audiobooks" },
  { href: "/likes", label: "Liked tracks" },
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

export function currentDestination(path: string): Destination {
  if (within(path, "/discover")) return "home";
  if (within(path, "/settings")) return "settings";
  return currentSection(path) ? "library" : null;
}
