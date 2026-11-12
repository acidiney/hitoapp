import type { Schema } from "../../../shared/schema";
import { useReportUnsaved } from "../../../shared/unsavedChanges";
import Button from "../../../shared/ui/Button";
import SectionWarning from "../../../shared/ui/SectionWarning";
import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";
import Switch from "../../../shared/ui/Switch";
import OrderHandle from "../../settings/components/OrderHandle";
import { setting } from "../../settings/model/settingsCatalog";
import { entryKey, useSourcePreferences } from "../hooks/useSourcePreferences";

type Entry = Schema<"SourcePreferenceEntry">;

export default function SourcePreferences({ account }: { account: string }) {
  return <PreferencesForm key={account} {...useSourcePreferences(account)} />;
}

export function describeSource(
  entry: Entry,
  connections: Schema<"PluginSettings"> | null,
) {
  if (entry.kind === "local_files")
    return { label: "Local files", hint: "Audio in your music folders" };
  if (entry.kind === "saved_copies")
    return { label: "Saved copies", hint: "Copies Hito saved for you" };
  const installation = connections?.installations.find(
    (item) => item.id === entry.installation_id,
  );
  const service = connections?.services.find(
    (item) => item.id === installation?.service_id,
  );
  return { label: service?.name ?? "Unavailable provider", hint: "Plugin" };
}

function PreferencesForm(preferences: ReturnType<typeof useSourcePreferences>) {
  useReportUnsaved(preferences.dirty);
  const { entries } = preferences;
  return (
    <SettingsCard
      setting={setting("source-preference")}
      description="Which source plays first when a track has several. A source you pick for one track always wins."
      busy={preferences.pending}
    >
      {!preferences.loaded && preferences.pending && (
        <p role="status">Loading source preference…</p>
      )}
      {preferences.error && (
        <SectionWarning tone="error" title={preferences.error}>
          <Button disabled={preferences.pending} onClick={preferences.reload}>
            Reload source preference
          </Button>
        </SectionWarning>
      )}
      {preferences.loaded && (
        <>
          <ol className="provider-order" aria-label="Source order">
            {entries.map((entry, index) => {
              const { label, hint } = describeSource(
                entry,
                preferences.connections,
              );
              return (
                <li key={entryKey(entry)}>
                  <SettingRow
                    leading={
                      <OrderHandle
                        label={label}
                        position={index + 1}
                        items={[
                          {
                            label: "Move up",
                            disabled: preferences.pending || index === 0,
                            onSelect: () => preferences.move(entry, -1),
                          },
                          {
                            label: "Move down",
                            disabled:
                              preferences.pending ||
                              index === entries.length - 1,
                            onSelect: () => preferences.move(entry, 1),
                          },
                        ]}
                      />
                    }
                    title={<h4>{label}</h4>}
                    hint={
                      entry.kind === "provider" ? `${hint} · connected` : hint
                    }
                  />
                </li>
              );
            })}
          </ol>
          <Switch
            label="Try the next source if one fails"
            hint="Up to six sources are tried for one play."
            disabled={preferences.pending}
            checked={preferences.fallback}
            onChange={preferences.setFallback}
          />
          {preferences.dirty && (
            <div className="settings-save">
              <Button
                variant="action"
                disabled={preferences.pending}
                onClick={() => void preferences.save()}
              >
                Save source preference
              </Button>
            </div>
          )}
          {preferences.pending && (
            <p role="status">Saving source preference…</p>
          )}
          {preferences.saved && <p role="status">Source preference saved.</p>}
        </>
      )}
    </SettingsCard>
  );
}
