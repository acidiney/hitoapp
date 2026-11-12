import type { SectionId } from "./sections";

export type SettingId =
  | "account"
  | "display-name"
  | "session"
  | "change-password"
  | "open-sessions"
  | "screen-lock"
  | "pin"
  | "lock-idle"
  | "lock-now"
  | "listening-history"
  | "automatic-lookups"
  | "lyrics"
  | "community"
  | "community-rankings"
  | "libraries"
  | "music-folders"
  | "metadata-providers"
  | "your-plugins"
  | "connect-service"
  | "plugin-services"
  | "playback-engine"
  | "streaming-quality"
  | "source-preference"
  | "accounts-access"
  | "shared-corrections"
  | "about-hito";

export type SettingEntry = {
  id: SettingId;
  section: SectionId;
  label: string;
  description?: string;
  keywords?: string;
  within?: string;
  admin?: boolean;
};

const ENTRIES: SettingEntry[] = [
  {
    id: "account",
    section: "profile",
    label: "You",
    keywords:
      "account username role administrator member photo avatar who am i",
  },
  {
    id: "display-name",
    section: "profile",
    label: "Display name",
    description:
      "An optional name shown in place of your username in the menu and account lists. It never changes how you sign in.",
    keywords: "name nickname profile username rename",
  },
  {
    id: "session",
    section: "profile",
    label: "Sign out",
    keywords: "sign out log out logout lock now pin",
  },
  {
    id: "change-password",
    section: "profile",
    label: "Change password",
    description:
      "Changing your password signs out every other session. This device stays signed in.",
    keywords: "current new repeat",
  },
  {
    id: "open-sessions",
    section: "profile",
    label: "Devices signed in",
    description:
      "Every browser or device signed in to your account. Revoke one you do not recognise.",
    keywords: "sign out other sessions devices revoke last active",
  },
  {
    id: "screen-lock",
    section: "privacy",
    label: "Screen lock",
    description:
      "A 4 to 8 digit PIN locks this session while you step away. It never replaces your password: five wrong PINs sign you out, and changing or removing it needs your password.",
    keywords: "pin idle timeout lock",
  },
  {
    id: "pin",
    section: "privacy",
    label: "PIN",
    within: "Screen lock",
    keywords: "set change remove four digits password",
  },
  {
    id: "lock-idle",
    section: "privacy",
    label: "Lock when idle",
    within: "Screen lock",
    keywords: "pin timeout minutes inactive",
  },
  {
    id: "lock-now",
    section: "privacy",
    label: "Lock now",
    within: "Screen lock",
    keywords: "pin step away",
  },
  {
    id: "listening-history",
    section: "privacy",
    label: "Listening history",
    description:
      "Choose whether Hito remembers what you play, or delete what it has saved. Only this account is affected.",
    keywords: "history recently played pause delete clear erase privacy",
  },
  {
    id: "automatic-lookups",
    section: "privacy",
    label: "What Hito sends out",
    keywords: "metadata online artist profiles background musicbrainz",
  },
  {
    id: "lyrics",
    section: "privacy",
    label: "Load lyrics automatically",
    keywords: "automatic lookup words song",
  },
  {
    id: "community",
    section: "privacy",
    label: "Community",
    description:
      "Your playlists, likes and listening history belong to your account. Choose separately what Hito may share. Changes take effect after saving. Reporting requires a configured community connection, and playback continues when the community service is unavailable.",
    keywords: "share sharing reporting consent",
  },
  {
    id: "community-rankings",
    section: "privacy",
    label: "Use community source rankings",
    description:
      "Use community results to choose among suitable available music sources. Your saved sources and personal source choices keep priority. This requires a configured community service; receiving rankings does not enable playback reporting.",
    keywords: "sources consent",
  },
  {
    id: "libraries",
    section: "music",
    label: "Libraries you can listen to",
    description:
      "Libraries this account can listen to. Your originals stay exactly where they are.",
    keywords: "collection personal shared",
  },
  {
    id: "music-folders",
    section: "music",
    label: "Music folders",
    keywords: "add folder scan remove directory path",
    admin: true,
  },
  {
    id: "metadata-providers",
    section: "music",
    label: "Artist details",
    description:
      "Control providers for artist profiles. File tags and your personal edits are kept.",
    keywords: "priority order add provider plugin musicbrainz",
  },
  {
    id: "your-plugins",
    section: "plugins",
    label: "Your plugins",
    description:
      "Credentials stay private to your account. Search connected music services in Discover.",
    keywords: "installed connection remove check",
  },
  {
    id: "connect-service",
    section: "plugins",
    label: "Connect a service",
    description:
      "Pick a service your administrator offers, review what it declares, then connect your account.",
    keywords: "plugin install add",
  },
  {
    id: "plugin-services",
    section: "plugins",
    label: "Services for this server",
    description:
      "Offer trusted plugin services for accounts to connect. Hito will contact these addresses from the server and get their names automatically.",
    keywords: "service address offer",
    admin: true,
  },
  {
    id: "playback-engine",
    section: "playback",
    label: "This device",
    keywords: "desktop app browser output audio source rankings",
  },
  {
    id: "streaming-quality",
    section: "playback",
    label: "Streaming quality",
    keywords: "audio bitrate original high data saver AAC browser bandwidth",
  },
  {
    id: "source-preference",
    section: "playback",
    label: "Source order",
    description:
      "Which source plays first when a track has several. A source you choose for one track always wins.",
    keywords:
      "source order priority provider local files saved copies fallback next source fails",
  },
  {
    id: "accounts-access",
    section: "accounts",
    label: "People",
    description: "People who can sign in to this server.",
    keywords: "create account delete grant access member administrator users",
    admin: true,
  },
  {
    id: "shared-corrections",
    section: "accounts",
    label: "Shared artist corrections",
    description:
      "Corrections an administrator applied for every account. Withdrawing one restores the reviews it hid; accounts that reviewed the recording again keep their own choice.",
    keywords: "artist withdraw",
    admin: true,
  },
  {
    id: "about-hito",
    section: "about",
    label: "About",
    keywords: "version libraries server update check upgrade release",
  },
];

export function settingEntries(): readonly SettingEntry[] {
  return ENTRIES;
}

export function setting(id: SettingId): SettingEntry {
  const found = ENTRIES.find((entry) => entry.id === id);
  if (!found) throw new Error(`Unknown setting: ${id}`);
  return found;
}
