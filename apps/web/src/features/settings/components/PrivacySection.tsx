import type { Schema } from "../../../shared/schema";

import CommunityRankingSettings from "../../community/components/CommunityRankingSettings";
import CommunitySettings from "../../community/components/CommunitySettings";
import ListeningHistorySettings from "../../history/components/ListeningHistorySettings";
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
      <ListeningHistorySettings
        key={`listening-history:${user.id}`}
        account={user.id}
      />
      <AutomaticLookupsSettings
        key={`automatic-lookups:${user.id}`}
        account={user.id}
      >
        <LyricsAutoSetting key={`lyrics-auto:${user.id}`} account={user.id} />
      </AutomaticLookupsSettings>
      <CommunitySettings
        key={`community-preferences:${user.id}`}
        account={user.id}
      >
        <CommunityRankingSettings
          key={`community-rankings:${user.id}`}
          account={user.id}
        />
      </CommunitySettings>
    </>
  );
}
