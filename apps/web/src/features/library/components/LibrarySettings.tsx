import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import { Link } from "@inertiajs/react";
import { Library, Plug } from "lucide-react";

import Listbox from "../../../shared/ui/Listbox";
import SectionWarning from "../../../shared/ui/SectionWarning";
import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";
import MusicFolders from "./MusicFolders";
import { sectionHref } from "../../settings/model/sections";
import { setting } from "../../settings/model/settingsCatalog";

export default function LibrarySettings({
  user,
  libraries,
}: Schema<"SettingsPageProps">) {
  const shared = libraries.filter((library) => !library.personal);
  const [selected, select] = useState(shared[0]?.id);
  const library = shared.find((item) => item.id === selected) ?? shared[0];
  return (
    <>
      {user.admin && library && (
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
        </>
      )}
      <SettingsCard setting={setting("libraries")}>
        {libraries.map((item) => (
          <SettingRow
            key={item.id}
            leading={
              <span className="setting-line-tile" aria-hidden="true">
                {item.personal ? <Plug size={18} /> : <Library size={18} />}
              </span>
            }
            title={item.personal ? "Your plugin library" : item.name}
            hint={
              item.personal
                ? "Tracks from your connected plugins"
                : "Shared on this server · read-only"
            }
            action={
              item.personal ? (
                <Link
                  className="setting-line-link"
                  href={sectionHref("plugins")}
                >
                  Open Plugins
                </Link>
              ) : undefined
            }
          />
        ))}
        {!libraries.length && (
          <SectionWarning title="No libraries are connected to this account yet.">
            Ask your administrator for access.
          </SectionWarning>
        )}
      </SettingsCard>
    </>
  );
}
