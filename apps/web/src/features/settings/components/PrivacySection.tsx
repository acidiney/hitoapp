import type { Schema } from "../../../shared/schema";

import CommunitySettings from "../../community/components/CommunitySettings";
import CommunityRankingSettings from "../../community/components/CommunityRankingSettings";
import LyricsAutoSetting from "../../lyrics/components/LyricsAutoSetting";
import AutomaticLookupsSettings from "../../metadata/components/AutomaticLookupsSettings";
import ScreenLockSettings from "../../session/components/ScreenLockSettings";

export default function PrivacySection({
  user,
  lock,
}: Schema<"SettingsPageProps">) {
  return (
    <>
      <ScreenLockSettings
        key={`screen-lock:${user.id}:${lock.pin_enabled}`}
        account={user.id}
        lock={lock}
      />
      <AutomaticLookupsSettings
        key={`automatic-lookups:${user.id}`}
        account={user.id}
      />
      <LyricsAutoSetting key={`lyrics-auto:${user.id}`} account={user.id} />
      <CommunitySettings
        key={`community-preferences:${user.id}`}
        account={user.id}
      />
      <CommunityRankingSettings
        key={`community-rankings:${user.id}`}
        account={user.id}
      />
    </>
  );
}
