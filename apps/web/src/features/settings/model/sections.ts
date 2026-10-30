export type SectionId =
  | "profile"
  | "privacy"
  | "music"
  | "plugins"
  | "playback"
  | "downloads"
  | "accounts"
  | "updates"
  | "about";

export type SettingsSection = {
  id: SectionId;
  label: string;
  description: string;
  admin?: boolean;
};

export type SectionGroup = { label: string; sections: SettingsSection[] };

export const DEFAULT_SECTION: SectionId = "music";

const GROUPS: SectionGroup[] = [
  {
    label: "Your account",
    sections: [
      {
        id: "profile",
        label: "Profile",
        description: "Who you are on this server.",
      },
      {
        id: "privacy",
        label: "Privacy & lock",
        description:
          "What stays with you, and how you protect this session when you step away.",
      },
    ],
  },
  {
    label: "Listening",
    sections: [
      {
        id: "music",
        label: "Music & providers",
        description:
          "Where your audio and metadata come from. The order decides which provider is used first.",
      },
      {
        id: "plugins",
        label: "Plugins",
        description:
          "Connect services offered by your administrator. Each account keeps its own connections.",
      },
      {
        id: "playback",
        label: "Playback",
        description: "How this device plays music.",
      },
      {
        id: "downloads",
        label: "Downloads & storage",
        description: "Where your music lives and how it is kept.",
      },
    ],
  },
  {
    label: "Server",
    sections: [
      {
        id: "accounts",
        label: "Accounts",
        description:
          "Each account has private playlists, likes and listening history.",
        admin: true,
      },
      {
        id: "updates",
        label: "Updates",
        description: "Keep this server current.",
      },
      {
        id: "about",
        label: "About",
        description: "What this server is running.",
      },
    ],
  },
];

const LEGACY: Record<string, SectionId> = {
  library: "music",
  metadata: "music",
};

export function visibleGroups(admin: boolean): SectionGroup[] {
  return GROUPS.map((group) => ({
    ...group,
    sections: group.sections.filter((section) => admin || !section.admin),
  }));
}

export function resolveSection(
  requested: string | null,
  admin: boolean,
): SectionId {
  if (requested === null) return DEFAULT_SECTION;
  const id = Object.hasOwn(LEGACY, requested) ? LEGACY[requested] : requested;
  const match = visibleGroups(admin)
    .flatMap((group) => group.sections)
    .find((section) => section.id === id);
  return match?.id ?? DEFAULT_SECTION;
}

export function sectionMeta(id: SectionId): SettingsSection {
  const found = GROUPS.flatMap((group) => group.sections).find(
    (section) => section.id === id,
  );
  if (!found) throw new Error(`Unknown settings section: ${id}`);
  return found;
}

export function sectionHref(id: SectionId): string {
  return `/settings?section=${id}`;
}

export function readLocation(
  url: string,
  admin: boolean,
): { section: SectionId; chosen: boolean; requested: string | null } {
  const requested = new URL(url, "http://hito.local").searchParams.get(
    "section",
  );
  return {
    section: resolveSection(requested, admin),
    chosen: requested !== null,
    requested,
  };
}
