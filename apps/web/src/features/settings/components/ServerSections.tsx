import { Link } from "@inertiajs/react";
import type { SettingsPageProps } from "../../../shared/types";
import HealthDot from "../../../shared/ui/HealthDot";
import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";
import { sectionHref } from "../sections";

export function PlaybackSection({ native }: { native: boolean }) {
  return (
    <SettingsCard title="Playback engine">
      <SettingRow
        title="Playback"
        hint={
          native
            ? "Music plays through the desktop app on this computer."
            : "Music plays in this browser."
        }
        status={
          <HealthDot
            tone="success"
            label={native ? "Desktop app" : "This browser"}
          />
        }
      />
      <SettingRow
        title="Source rankings"
        hint="Choose whether community results help pick between sources."
        action={
          <Link className="button" href={sectionHref("privacy")}>
            Privacy & lock
          </Link>
        }
      />
    </SettingsCard>
  );
}

export function DownloadsSection({ admin }: { admin: boolean }) {
  return (
    <SettingsCard title="Downloads and storage">
      <SettingRow
        title="Downloads"
        hint="Start a download from a playlist or a track."
        action={
          <Link className="button" href="/playlists">
            Playlists
          </Link>
        }
      />
      <SettingRow
        title="Music folders"
        hint={
          admin
            ? "Folders are added, scanned and removed under Music & providers."
            : "Your administrator manages the music folders."
        }
        action={
          admin ? (
            <Link className="button" href={sectionHref("music")}>
              Open folders
            </Link>
          ) : undefined
        }
      />
    </SettingsCard>
  );
}

export function UpdatesSection() {
  return (
    <SettingsCard title="Update checks">
      <SettingRow
        title="Update checks"
        hint="Hito does not check for updates yet. Install a newer build on the server to update."
        status={<HealthDot tone="warning" label="Not available" />}
      />
    </SettingsCard>
  );
}

export function AboutSection({ user, libraries }: SettingsPageProps) {
  return (
    <SettingsCard title="About Hito">
      <SettingRow title="Hito" hint="A self-hosted music library and player." />
      <SettingRow title="Signed in as" hint={user.username} />
      <SettingRow
        title="Libraries"
        hint={`${libraries.length} ${libraries.length === 1 ? "library" : "libraries"} connected to this account.`}
      />
    </SettingsCard>
  );
}
