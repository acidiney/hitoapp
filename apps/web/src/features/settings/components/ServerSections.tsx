import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";

import HealthDot from "../../../shared/ui/HealthDot";
import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";
import { sectionHref } from "../model/sections";
import { setting } from "../model/settingsCatalog";
import SourcePreferences from "../../sources/components/SourcePreferences";

export function PlaybackSection({
  native,
  account,
}: {
  native: boolean;
  account: string;
}) {
  return (
    <>
      <SettingsCard setting={setting("playback-engine")}>
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
      <SourcePreferences account={account} />
    </>
  );
}

export function DownloadsSection({ admin }: { admin: boolean }) {
  return (
    <SettingsCard setting={setting("downloads-storage")}>
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
    <SettingsCard setting={setting("update-checks")}>
      <SettingRow
        title="Update checks"
        hint="Hito does not check for updates yet. Install a newer build on the server to update."
        status={<HealthDot tone="warning" label="Not available" />}
      />
    </SettingsCard>
  );
}

export function AboutSection({ user, libraries }: Schema<"SettingsPageProps">) {
  return (
    <SettingsCard setting={setting("about-hito")}>
      <SettingRow title="Hito" hint="A self-hosted music library and player." />
      <SettingRow title="Signed in as" hint={user.username} />
      <SettingRow
        title="Libraries"
        hint={`${libraries.length} ${libraries.length === 1 ? "library" : "libraries"} connected to this account.`}
      />
    </SettingsCard>
  );
}
