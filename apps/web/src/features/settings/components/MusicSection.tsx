import type { SettingsPageProps } from "../../../shared/types";
import LibrarySettings from "../../library/components/LibrarySettings";
import MetadataPreferences from "../../metadata/components/MetadataPreferences";

export default function MusicSection(props: SettingsPageProps) {
  return (
    <>
      <LibrarySettings {...props} />
      <MetadataPreferences key={props.user.id} account={props.user.id} />
    </>
  );
}
