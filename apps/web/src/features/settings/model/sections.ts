export type SectionId =
  | "profile"
  | "privacy"
  | "music"
  | "plugins"
  | "playback"
  | "accounts"
  | "about";

export type SettingsSection = {
  id: SectionId;
  label: string;
  description: string;
  admin?: boolean;
  tab?: false;
};

export const DEFAULT_SECTION: SectionId = "profile";

const SECTIONS: SettingsSection[] = [
  {
    id: "profile",
    label: "Profile",
    description: "Name, photo, password, devices",
  },
  {
    id: "privacy",
    label: "Privacy & lock",
    description: "PIN, history, what Hito sends out",
  },
  {
    id: "music",
    label: "Music",
    description: "Folders, libraries, artist details",
  },
  {
    id: "plugins",
    label: "Plugins",
    description: "Connect services offered by your administrator",
    tab: false,
  },
  {
    id: "playback",
    label: "Playback",
    description: "Quality and source order",
  },
  {
    id: "accounts",
    label: "Accounts",
    description: "People on this server",
    admin: true,
  },
  {
    id: "about",
    label: "About",
    description: "Version and updates",
  },
];

const LEGACY: Record<string, SectionId> = {
  library: "music",
  metadata: "music",
  downloads: "music",
  updates: "about",
};

export function visibleSections(admin: boolean): SettingsSection[] {
  return SECTIONS.filter((section) => admin || !section.admin);
}

export function tabSections(admin: boolean): SettingsSection[] {
  return visibleSections(admin).filter((section) => section.tab !== false);
}

export function resolveSection(
  requested: string | null,
  admin: boolean,
): SectionId {
  if (requested === null) return DEFAULT_SECTION;
  const id = Object.hasOwn(LEGACY, requested) ? LEGACY[requested] : requested;
  const match = visibleSections(admin).find((section) => section.id === id);
  return match?.id ?? DEFAULT_SECTION;
}

export function sectionMeta(id: SectionId): SettingsSection {
  const found = SECTIONS.find((section) => section.id === id);
  if (!found) throw new Error(`Unknown settings section: ${id}`);
  return found;
}

export function sectionHref(id: SectionId): string {
  return `/settings?section=${id}`;
}

export function readLocation(
  url: string,
  admin: boolean,
): {
  section: SectionId;
  chosen: boolean;
  requested: string | null;
  anchor: string | null;
} {
  const parsed = new URL(url, "http://hito.local");
  const requested = parsed.searchParams.get("section");
  const anchor = parsed.hash.startsWith("#setting-")
    ? parsed.hash.slice(1)
    : null;
  return {
    anchor,
    section: resolveSection(requested, admin),
    chosen: requested !== null,
    requested,
  };
}
