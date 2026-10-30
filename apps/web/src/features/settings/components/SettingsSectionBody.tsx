import type { Schema } from "../../../shared/schema";

import PluginSettings from "../../plugins/components/PluginSettings";
import type { SectionId } from "../model/sections";
import AccountsSection from "./AccountsSection";
import MusicSection from "./MusicSection";
import PrivacySection from "./PrivacySection";
import ProfileSection from "./ProfileSection";
import {
  AboutSection,
  DownloadsSection,
  PlaybackSection,
  UpdatesSection,
} from "./ServerSections";

export default function SettingsSectionBody({
  section,
  ...props
}: Schema<"SettingsPageProps"> & { section: SectionId }) {
  switch (section) {
    case "profile":
      return <ProfileSection {...props} />;
    case "privacy":
      return <PrivacySection {...props} />;
    case "music":
      return <MusicSection {...props} />;
    case "plugins":
      return (
        <PluginSettings account={props.user.id} admin={props.user.admin} />
      );
    case "playback":
      return <PlaybackSection native={props.native_playback} />;
    case "downloads":
      return <DownloadsSection admin={props.user.admin} />;
    case "accounts":
      return props.user.admin ? <AccountsSection {...props} /> : null;
    case "updates":
      return <UpdatesSection />;
    case "about":
      return <AboutSection {...props} />;
  }
}
