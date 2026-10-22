import type { FormEvent } from "react";
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
    <section
      className="settings-panel"
      aria-label="Metadata preferences"
      aria-busy={preferences.pending}
    >
      <div className="settings-intro">
        <h2>Metadata providers</h2>
        <p>
          Control providers for artist profiles. File tags and your personal
          edits are kept.
        </p>
      </div>
      {!preferences.loaded && preferences.pending && (
        <p role="status">Loading metadata preferences…</p>
      )}
      {preferences.error && (
        <div role="alert" className="form-error">
          <p>{preferences.error}</p>
          <button
            className="button"
            disabled={preferences.pending}
            onClick={preferences.reload}
          >
            Reload metadata preferences
          </button>
        </div>
      )}
      {preferences.loaded && (
        <form onSubmit={submit}>
          <fieldset disabled={preferences.pending}>
            <label>
              <input
                type="checkbox"
                checked={preferences.enabled}
                onChange={(event) =>
                  preferences.setEnabled(event.target.checked)
                }
              />
              Use metadata providers
            </label>
            <MetadataProviderList
              providers={preferences.providers}
              connections={preferences.connections}
              pending={preferences.pending}
              canAdd={preferences.canAddProvider}
              onAdd={preferences.addProvider}
              onRemove={preferences.removeProvider}
              onMove={preferences.moveProvider}
            />
            <button
              className="button"
              disabled={
                !preferences.dirty ||
                (preferences.enabled && !preferences.providers.length)
              }
            >
              Save metadata preferences
            </button>
          </fieldset>
          {preferences.pending && (
            <p role="status">Saving metadata preferences…</p>
          )}
          {preferences.saved && (
            <p role="status">Metadata preferences saved.</p>
          )}
        </form>
      )}
    </section>
  );
}
