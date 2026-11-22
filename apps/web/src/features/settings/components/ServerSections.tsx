import type { Schema } from "../../../shared/schema";

import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";
import { setting } from "../model/settingsCatalog";
import SourcePreferences from "../../sources/components/SourcePreferences";
import BackgroundPreparation from "../../sources/components/BackgroundPreparation";
import StreamingQualitySettings from "../../playback/components/StreamingQualitySettings";
import AppPlaceSetting from "../../place/components/AppPlaceSetting";
import KeepPlayedTracks from "../../downloads/components/KeepPlayedTracks";

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
          title="Plays in"
          hint={native ? "The desktop app on this computer" : "This browser"}
        />
        {!native && <StreamingQualitySettings />}
      </SettingsCard>
      <SourcePreferences account={account} />
      <BackgroundPreparation account={account} />
      <KeepPlayedTracks account={account} native={native} />
    </>
  );
}

export function AboutSection({ libraries, user }: Schema<"SettingsPageProps">) {
  return (
    <SettingsCard setting={setting("about-hito")}>
      <SettingRow title="Hito" hint="A self-hosted music library and player." />
      <SettingRow
        title="Server"
        hint={window.location.host}
        status={`${libraries.length} ${libraries.length === 1 ? "library" : "libraries"}`}
      />
      <AppPlaceSetting account={user.id} />
      <SettingRow
        title="Updates"
        hint="Hito does not check for updates yet. Install a newer build on the server to update."
      />
    </SettingsCard>
  );
}
