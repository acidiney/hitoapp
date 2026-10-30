import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import { Folder } from "lucide-react";

import SectionWarning from "../../../shared/ui/SectionWarning";
import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";
import MusicFolders from "./MusicFolders";

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
          <label className="library-chooser">
            Library
            <select
              value={library.id}
              onChange={(event) => select(event.target.value)}
            >
              {shared.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.name}
                </option>
              ))}
            </select>
          </label>
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
    <SettingsCard
      title="Connected libraries"
      description="Libraries this account can listen to. Your originals stay exactly where they are."
    >
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
