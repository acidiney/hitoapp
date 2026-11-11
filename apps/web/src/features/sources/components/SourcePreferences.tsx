import type { Schema } from "../../../shared/schema";
import type { FormEvent } from "react";
import { ArrowDown, ArrowUp } from "lucide-react";
import { useReportUnsaved } from "../../../shared/unsavedChanges";
import Button from "../../../shared/ui/Button";
import HealthDot from "../../../shared/ui/HealthDot";
import SectionWarning from "../../../shared/ui/SectionWarning";
import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";
import Switch from "../../../shared/ui/Switch";
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
  function submit(event: FormEvent) {
    event.preventDefault();
    void preferences.save();
  }
  const { entries } = preferences;
  return (
    <SettingsCard
      setting={setting("source-preference")}
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
        <form onSubmit={submit}>
          <fieldset disabled={preferences.pending}>
            <ol className="provider-order" aria-label="Source order">
              {entries.map((entry, index) => {
                const { label, hint } = describeSource(
                  entry,
                  preferences.connections,
                );
                return (
                  <li key={entryKey(entry)}>
                    <SettingRow
                      title={<h4>{label}</h4>}
                      hint={
                        <>
                          <span>{hint}</span>
                          <span>{`Priority ${index + 1}`}</span>
                        </>
                      }
                      status={
                        entry.kind === "provider" ? (
                          <HealthDot tone="success" label="Connected" />
                        ) : undefined
                      }
                      wide
                      action={
                        <>
                          <button
                            type="button"
                            className="icon-button"
                            aria-label={`Move up ${label}`}
                            title="Move up"
                            disabled={preferences.pending || index === 0}
                            onClick={() => preferences.move(entry, -1)}
                          >
                            <ArrowUp size={18} aria-hidden="true" />
                          </button>
                          <button
                            type="button"
                            className="icon-button"
                            aria-label={`Move down ${label}`}
                            title="Move down"
                            disabled={
                              preferences.pending ||
                              index === entries.length - 1
                            }
                            onClick={() => preferences.move(entry, 1)}
                          >
                            <ArrowDown size={18} aria-hidden="true" />
                          </button>
                        </>
                      }
                    />
                  </li>
                );
              })}
            </ol>
            <Switch
              label="Try the next source if one fails"
              hint="Up to six sources are tried for one play. Turn this off to stop at the first one."
              disabled={preferences.pending}
              checked={preferences.fallback}
              onChange={preferences.setFallback}
            />
            <div className="form-actions">
              <Button
                type="submit"
                variant="primary"
                disabled={!preferences.dirty}
              >
                Save source preference
              </Button>
            </div>
          </fieldset>
          {preferences.pending && (
            <p role="status">Saving source preference…</p>
          )}
          {preferences.saved && <p role="status">Source preference saved.</p>}
        </form>
      )}
    </SettingsCard>
  );
}
