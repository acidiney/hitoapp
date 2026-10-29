import { type FormEvent } from "react";
import { useReportUnsaved } from "../../../shared/unsavedChanges";
import Button from "../../../shared/ui/Button";
import SectionWarning from "../../../shared/ui/SectionWarning";
import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";
import Switch from "../../../shared/ui/Switch";
import { useCommunityPreferences } from "../hooks/useCommunityPreferences";
import { useCommunityCredentialReset } from "../hooks/useCommunityCredentialReset";

export default function CommunitySettings({ account }: { account: string }) {
  const preferences = useCommunityPreferences(account);
  const credentials = useCommunityCredentialReset(account);
  const busy = preferences.pending !== null || credentials.pending;
  useReportUnsaved(preferences.dirty);
  function submit(event: FormEvent) {
    event.preventDefault();
    void preferences.save();
  }
  return (
    <SettingsCard
      title="Community"
      description="Your playlists, likes and listening history belong to your account. Choose separately what Hito may share. Changes take effect after saving. Reporting requires a configured community connection, and playback continues when the community service is unavailable."
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
        <form onSubmit={submit}>
          <fieldset disabled={busy}>
            <Switch
              label="Playback reporting"
              disabled={busy}
              hint="Allow successful playback of supported public sources to inform community rankings. Audio, paths, titles, credentials and stream URLs stay private."
              checked={preferences.playbackReporting}
              onChange={(value) =>
                preferences.edit("playback_reporting", value)
              }
            />
            <Switch
              label="Metadata corrections"
              disabled={busy}
              hint="Allow reviewed public matching corrections to be shared. Correction submission is not available yet."
              checked={preferences.metadataCorrections}
              onChange={(value) =>
                preferences.edit("metadata_corrections", value)
              }
            />
            <div className="form-actions">
              <Button
                type="submit"
                variant="primary"
                disabled={!preferences.dirty}
              >
                Save community preferences
              </Button>
              <Button
                disabled={!preferences.dirty}
                onClick={preferences.cancel}
              >
                Cancel changes
              </Button>
            </div>
          </fieldset>
          {preferences.pending === "save" && (
            <p role="status">Saving community preferences…</p>
          )}
          {preferences.saved && (
            <p role="status">Community preferences saved.</p>
          )}
        </form>
      )}
      {preferences.loaded && (
        <SettingRow
          title="Community connection recovery"
          hint="Clear your account's saved community credentials and cancel unsent reports. Your privacy choices and accepted contributions are kept."
          action={
            <Button disabled={busy} onClick={() => void credentials.reset()}>
              Reset community credentials
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
      )}
    </SettingsCard>
  );
}
