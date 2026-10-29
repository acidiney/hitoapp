import type { FormEvent } from "react";
import Button from "../../../shared/ui/Button";
import SectionWarning from "../../../shared/ui/SectionWarning";
import SettingsCard from "../../../shared/ui/SettingsCard";
import Switch from "../../../shared/ui/Switch";
import { useMetadataPreferences } from "../hooks/useMetadataPreferences";
import MetadataProviderList from "./MetadataProviderList";

export default function MetadataPreferences({ account }: { account: string }) {
  return <PreferencesForm key={account} {...useMetadataPreferences(account)} />;
}

function PreferencesForm(
  preferences: ReturnType<typeof useMetadataPreferences>,
) {
  function submit(event: FormEvent) {
    event.preventDefault();
    void preferences.save();
  }
  return (
    <SettingsCard
      title="Metadata providers"
      description="Control providers for artist profiles. File tags and your personal edits are kept."
      busy={preferences.pending}
    >
      {!preferences.loaded && preferences.pending && (
        <p role="status">Loading metadata preferences…</p>
      )}
      {preferences.error && (
        <SectionWarning tone="error" title={preferences.error}>
          <Button disabled={preferences.pending} onClick={preferences.reload}>
            Reload metadata preferences
          </Button>
        </SectionWarning>
      )}
      {preferences.loaded && (
        <form onSubmit={submit}>
          <fieldset disabled={preferences.pending}>
            <Switch
              label="Use metadata providers"
              disabled={preferences.pending}
              checked={preferences.enabled}
              onChange={preferences.setEnabled}
            />
            <MetadataProviderList
              providers={preferences.providers}
              connections={preferences.connections}
              pending={preferences.pending}
              canAdd={preferences.canAddProvider}
              onAdd={preferences.addProvider}
              onRemove={preferences.removeProvider}
              onMove={preferences.moveProvider}
            />
            <div className="form-actions">
              <Button
                type="submit"
                variant="primary"
                disabled={
                  !preferences.dirty ||
                  (preferences.enabled && !preferences.providers.length)
                }
              >
                Save metadata preferences
              </Button>
            </div>
          </fieldset>
          {preferences.pending && (
            <p role="status">Saving metadata preferences…</p>
          )}
          {preferences.saved && (
            <p role="status">Metadata preferences saved.</p>
          )}
        </form>
      )}
    </SettingsCard>
  );
}
