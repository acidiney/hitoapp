import { useState, type ReactNode } from "react";
import { useReportUnsaved } from "../../../shared/unsavedChanges";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import SectionWarning from "../../../shared/ui/SectionWarning";
import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";
import Switch from "../../../shared/ui/Switch";
import { useCommunityPreferences } from "../hooks/useCommunityPreferences";
import { useCommunityCredentialReset } from "../hooks/useCommunityCredentialReset";
import { setting } from "../../settings/model/settingsCatalog";

export default function CommunitySettings({
  account,
  children,
}: {
  account: string;
  children?: ReactNode;
}) {
  const preferences = useCommunityPreferences(account);
  const credentials = useCommunityCredentialReset(account);
  const [confirming, setConfirming] = useState(false);
  const busy = preferences.pending !== null || credentials.pending;
  useReportUnsaved(preferences.dirty);
  return (
    <SettingsCard
      setting={setting("community")}
      description="Needs a community connection on this server. Music keeps playing when it is unavailable."
      busy={busy}
    >
      {!preferences.loaded && preferences.pending === "load" && (
        <p role="status">Loading community preferences…</p>
      )}
      {preferences.error && (
        <SectionWarning tone="error" title={preferences.error}>
          <Button disabled={busy} onClick={preferences.reload}>
            Reload community preferences
          </Button>
        </SectionWarning>
      )}
      {preferences.loaded && (
        <>
          <Switch
            label="Report successful plays"
            disabled={busy}
            hint="Helps rank public sources. Audio, paths, titles and credentials stay private."
            checked={preferences.playbackReporting}
            onChange={(value) => preferences.edit("playback_reporting", value)}
          />
          <Switch
            label="Share metadata corrections"
            disabled={busy}
            hint="Not available yet. Your choice is kept for when it is."
            checked={preferences.metadataCorrections}
            onChange={(value) =>
              preferences.edit("metadata_corrections", value)
            }
          />
          {children}
          {preferences.dirty && (
            <div className="settings-save">
              <Button
                variant="action"
                disabled={busy}
                onClick={() => void preferences.save()}
              >
                Save community preferences
              </Button>
              <Button disabled={busy} onClick={preferences.cancel}>
                Cancel changes
              </Button>
            </div>
          )}
          {preferences.pending === "save" && (
            <p role="status">Saving community preferences…</p>
          )}
          {preferences.saved && (
            <p role="status">Community preferences saved.</p>
          )}
          <SettingRow
            id="community-reset"
            title="Reset community credentials"
            hint="Clears saved credentials and cancels unsent reports."
            action={
              <Button
                variant="link"
                disabled={busy}
                aria-label="Reset community credentials"
                onClick={() => setConfirming(true)}
              >
                Reset…
              </Button>
            }
          >
            {credentials.pending && (
              <p role="status">Resetting community credentials…</p>
            )}
            {credentials.error && (
              <p role="alert" className="form-error">
                {credentials.error}
              </p>
            )}
            {credentials.receipt && (
              <p role="status">
                Community credentials reset. Privacy choices and accepted
                contributions are kept. Unsent reports canceled:{" "}
                {credentials.receipt.reports_cancelled}.
              </p>
            )}
          </SettingRow>
        </>
      )}
      {confirming && (
        <Dialog
          title="Reset community credentials?"
          description="Clears your account's saved community credentials and cancels unsent reports. Your privacy choices and accepted contributions are kept."
          busy={credentials.pending}
          onClose={() => setConfirming(false)}
        >
          <div className="dialog-actions">
            <Button onClick={() => setConfirming(false)}>Cancel</Button>
            <Button
              variant="destructive"
              disabled={credentials.pending}
              onClick={async () => {
                await credentials.reset();
                setConfirming(false);
              }}
            >
              Reset credentials
            </Button>
          </div>
        </Dialog>
      )}
    </SettingsCard>
  );
}
