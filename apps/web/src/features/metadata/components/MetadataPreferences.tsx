import { useReportUnsaved } from "../../../shared/unsavedChanges";
import Button from "../../../shared/ui/Button";
import SectionWarning from "../../../shared/ui/SectionWarning";
import SettingsCard from "../../../shared/ui/SettingsCard";
import Switch from "../../../shared/ui/Switch";
import { useMetadataPreferences } from "../hooks/useMetadataPreferences";
import MetadataProviderList from "./MetadataProviderList";
import { setting } from "../../settings/model/settingsCatalog";

export default function MetadataPreferences({ account }: { account: string }) {
  return <PreferencesForm key={account} {...useMetadataPreferences(account)} />;
}

function PreferencesForm(
  preferences: ReturnType<typeof useMetadataPreferences>,
) {
  useReportUnsaved(preferences.dirty);
  return (
    <SettingsCard
      setting={setting("metadata-providers")}
      description="File tags and your own edits are always kept."
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
        <>
          <Switch
            label="Use metadata providers"
            hint="Fills in artist profiles from the providers below, in this order."
            disabled={preferences.pending}
            checked={preferences.enabled}
            onChange={preferences.setEnabled}
          />
          <MetadataProviderList
            providers={preferences.providers}
            connections={preferences.connections}
            enabled={preferences.enabled}
            pending={preferences.pending}
            canAdd={preferences.canAddProvider}
            onAdd={preferences.addProvider}
            onRemove={preferences.removeProvider}
            onMove={preferences.moveProvider}
          />
          {preferences.dirty && (
            <div className="settings-save">
              <Button
                variant="action"
                disabled={
                  preferences.pending ||
                  (preferences.enabled && !preferences.providers.length)
                }
                onClick={() => void preferences.save()}
              >
                Save metadata preferences
              </Button>
            </div>
          )}
          {preferences.pending && (
            <p role="status">Saving metadata preferences…</p>
          )}
          {preferences.saved && (
            <p role="status">Metadata preferences saved.</p>
          )}
        </>
      )}
    </SettingsCard>
  );
}
