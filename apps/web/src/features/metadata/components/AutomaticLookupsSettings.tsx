import SettingsCard from "../../../shared/ui/SettingsCard";
import Switch from "../../../shared/ui/Switch";
import {
  useAutomaticLookups,
  type AutomaticLookup,
} from "../hooks/useAutomaticLookups";

const SWITCHES: { key: AutomaticLookup; label: string; hint: string }[] = [
  {
    key: "automatic_artist_lookup",
    label: "Look up artists on MusicBrainz automatically",
    hint: "After a scan or import, Hito sends track codes (ISRC), artist page links and, when nothing else places a track, artist names to MusicBrainz. It never sends album titles, file paths or audio. On by default; turn off to stop all of this.",
  },
  {
    key: "automatic_artwork",
    label: "Fetch album artwork automatically",
    hint: "Album covers are requested from the Cover Art Archive, which receives only a MusicBrainz identifier. On by default.",
  },
  {
    key: "automatic_service_search",
    label: "Search connected services automatically",
    hint: "When your library has few matches, the search text is sent to your connected music services. On by default. Turn off to send it only when you ask.",
  },
];

export default function AutomaticLookupsSettings({
  account,
}: {
  account: string;
}) {
  const { loaded, choices, saving, error, change } =
    useAutomaticLookups(account);
  return (
    <SettingsCard title="Automatic lookups" busy={saving}>
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
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
    </SettingsCard>
  );
}
