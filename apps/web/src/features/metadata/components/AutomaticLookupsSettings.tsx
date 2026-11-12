import type { ReactNode } from "react";
import SettingsCard from "../../../shared/ui/SettingsCard";
import Switch from "../../../shared/ui/Switch";
import {
  useAutomaticLookups,
  type AutomaticLookup,
} from "../hooks/useAutomaticLookups";
import { setting } from "../../settings/model/settingsCatalog";

const SWITCHES: { key: AutomaticLookup; label: string; hint: string }[] = [
  {
    key: "automatic_artist_lookup",
    label: "Look up artists on MusicBrainz",
    hint: "Sends track codes, artist links and sometimes artist names. Never album titles, file paths or audio.",
  },
  {
    key: "automatic_artwork",
    label: "Fetch album covers",
    hint: "Asks the Cover Art Archive, using only a MusicBrainz identifier.",
  },
  {
    key: "automatic_service_search",
    label: "Search connected services",
    hint: "When your library has few matches, sends your search text to your plugins.",
  },
];

export default function AutomaticLookupsSettings({
  account,
  children,
}: {
  account: string;
  children?: ReactNode;
}) {
  const { loaded, choices, saving, error, change } =
    useAutomaticLookups(account);
  return (
    <SettingsCard
      setting={setting("automatic-lookups")}
      description="Each lookup is on until you turn it off."
      busy={saving}
    >
      {SWITCHES.map(({ key, label, hint }) => (
        <Switch
          key={key}
          label={label}
          hint={hint}
          checked={choices?.[key] ?? false}
          disabled={!loaded || saving}
          onChange={(value) => void change(key, value)}
        />
      ))}
      {children}
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
    </SettingsCard>
  );
}
