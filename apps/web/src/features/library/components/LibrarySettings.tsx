import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import { Folder } from "lucide-react";

import Listbox from "../../../shared/ui/Listbox";
import SectionWarning from "../../../shared/ui/SectionWarning";
import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";
import MusicFolders from "./MusicFolders";
import { setting } from "../../settings/model/settingsCatalog";

export default function LibrarySettings({
  user,
  libraries,
}: Schema<"SettingsPageProps">) {
  const shared = libraries.filter((library) => !library.personal);
  const [selected, select] = useState(shared[0]?.id);
  const library = shared.find((item) => item.id === selected) ?? shared[0];
  if (user.admin && library)
    return (
      <>
        {shared.length > 1 && (
          <div className="library-chooser">
            <Listbox
              label="Library"
              value={library.id}
              options={shared.map((item) => ({
                value: item.id,
                label: item.name,
              }))}
              onChange={select}
            />
          </div>
        )}
        <MusicFolders
          key={`${user.id}\0${library.id}`}
          account={user.id}
          library={library}
        />
        {libraries.some((item) => item.personal) && (
          <p className="muted">
            Your personal provider library is managed in Plugins.
          </p>
        )}
      </>
    );
  return (
    <SettingsCard setting={setting("libraries")}>
      {libraries.map((item) => (
        <SettingRow
          key={item.id}
          title={
            <span className="setting-line-icon">
              <Folder size={18} aria-hidden="true" />
              {item.name}
            </span>
          }
          hint={
            item.personal
              ? "Your connected plugins"
              : "Managed by your administrator"
          }
        />
      ))}
      {!libraries.length && (
        <SectionWarning title="No libraries are connected to this account yet.">
          Ask your administrator for access.
        </SectionWarning>
      )}
    </SettingsCard>
  );
}
