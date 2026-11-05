import { useState } from "react";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import SectionWarning from "../../../shared/ui/SectionWarning";
import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";
import Switch from "../../../shared/ui/Switch";
import { setting } from "../../settings/model/settingsCatalog";
import { useListeningHistory } from "../hooks/useListeningHistory";

export default function ListeningHistorySettings({
  account,
}: {
  account: string;
}) {
  const history = useListeningHistory(account);
  const [confirming, setConfirming] = useState(false);
  const busy = history.phase !== null;
  async function confirm() {
    await history.clear();
    setConfirming(false);
  }
  return (
    <SettingsCard setting={setting("listening-history")} busy={busy}>
      {history.phase === "load" && (
        <p role="status">Loading listening history settings…</p>
      )}
      {history.error && (
        <SectionWarning tone="error" title={history.error}>
          {!history.loaded && (
            <Button disabled={busy} onClick={history.reload}>
              Reload listening history settings
            </Button>
          )}
        </SectionWarning>
      )}
      {history.loaded && (
        <>
          <Switch
            label="Save listening history"
            hint="Hito remembers the tracks you play so History and Recently played can show them. Turn off to stop recording. Entries already saved stay until you delete them, and resume positions are still kept."
            checked={history.enabled}
            disabled={busy}
            onChange={(enabled) => void history.setEnabled(enabled)}
          />
          {!history.enabled && (
            <p role="status" className="muted">
              Listening history is paused. New plays are not added.
            </p>
          )}
          <SettingRow
            title="Delete listening history"
            hint="Removes every entry from History and clears Recently played. Likes, playlists, resume positions and downloads are not affected."
            action={
              <Button
                variant="destructive"
                disabled={busy}
                onClick={() => setConfirming(true)}
              >
                Delete listening history
              </Button>
            }
          >
            {history.phase === "delete" && (
              <p role="status">Deleting listening history…</p>
            )}
            {history.removed !== null && (
              <p role="status">
                Listening history deleted.{" "}
                {history.removed === 1
                  ? "1 entry removed."
                  : `${history.removed} entries removed.`}
              </p>
            )}
          </SettingRow>
        </>
      )}
      {confirming && (
        <Dialog
          title="Delete listening history?"
          description="Every entry in your History and Recently played is removed. This cannot be undone. Likes, playlists, resume positions and downloads are kept."
          busy={history.phase === "delete"}
          onClose={() => setConfirming(false)}
        >
          <div className="dialog-actions">
            <Button
              disabled={history.phase === "delete"}
              onClick={() => setConfirming(false)}
            >
              Cancel
            </Button>
            <Button
              variant="destructive"
              disabled={history.phase === "delete"}
              onClick={() => void confirm()}
            >
              Delete history
            </Button>
          </div>
        </Dialog>
      )}
    </SettingsCard>
  );
}
