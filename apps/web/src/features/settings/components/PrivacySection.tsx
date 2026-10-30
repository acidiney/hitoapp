import type { Schema } from "../../../shared/schema";

import CommunitySettings from "../../community/components/CommunitySettings";
import CommunityRankingSettings from "../../community/components/CommunityRankingSettings";
import SearchServicesSetting from "../../search/components/SearchServicesSetting";
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
      <SearchServicesSetting
        key={`search-services:${user.id}`}
        account={user.id}
      />
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
