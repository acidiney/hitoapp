import Button from "../../../shared/ui/Button";
import SectionWarning from "../../../shared/ui/SectionWarning";
import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";
import Switch from "../../../shared/ui/Switch";
import { setting } from "../../settings/model/settingsCatalog";
import { useAutomaticCopies } from "../hooks/useAutomaticCopies";
import { spaceText } from "../model/space";

const REMOVAL = "Turning this off removes these copies. Your Downloads stay.";

export default function KeepPlayedTracks({
  account,
  native,
}: {
  account: string;
  native: boolean;
}) {
  const automatic = useAutomaticCopies(account);
  const copies = automatic.copies;
  const busy = automatic.phase !== null;
  return (
    <SettingsCard setting={setting("keep-played-tracks")} busy={busy}>
      {automatic.phase === "load" && (
        <p role="status">Loading played tracks…</p>
      )}
      {automatic.error && (
        <SectionWarning tone="error" title={automatic.error}>
          {!copies && (
            <Button disabled={busy} onClick={automatic.reload}>
              Reload played tracks
            </Button>
          )}
        </SectionWarning>
      )}
      {copies && (
        <>
          <Switch
            id="keep-played-tracks"
            label="Keep played tracks"
            hint={
              native
                ? `Hito saves a private copy of tracks from services after you play them to the end, so they start without waiting next time. ${REMOVAL}`
                : `Hito saves a private copy of tracks from services after you play them, and of the next two in your queue, so they start without waiting. ${REMOVAL}`
            }
            checked={copies.keep_played_tracks}
            disabled={busy}
            onChange={(keep) => void automatic.setKeep(keep)}
          />
          {copies.keep_played_tracks && (
            <SettingRow
              title="Space used"
              hint={
                copies.budget_bytes === 0
                  ? "Your administrator has set no space for played tracks."
                  : `${spaceText(copies.used_bytes)} of ${spaceText(copies.budget_bytes)}`
              }
            />
          )}
        </>
      )}
    </SettingsCard>
  );
}
