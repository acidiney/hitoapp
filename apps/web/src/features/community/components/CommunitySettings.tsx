import { useId, type FormEvent } from "react";
import { useCommunityPreferences } from "../hooks/useCommunityPreferences";
import { useCommunityCredentialReset } from "../hooks/useCommunityCredentialReset";

export default function CommunitySettings({ account }: { account: string }) {
  const preferences = useCommunityPreferences(account);
  const credentials = useCommunityCredentialReset(account);
  const busy = preferences.pending !== null || credentials.pending;
  const id = useId();
  function submit(event: FormEvent) {
    event.preventDefault();
    void preferences.save();
  }
  return (
    <section
      className="settings-panel"
      aria-label="Community privacy"
      aria-busy={busy}
    >
      <div className="settings-intro">
        <h2>A private place to listen</h2>
        <p>
          Your playlists, likes, and listening history belong to your account.
        </p>
        <p>
          Choose separately what Hito may share. Changes take effect after
          saving.
        </p>
        <p>
          Your choices are saved on this server. Reporting requires a configured
          community connection. Playback continues when the community service is
          unavailable.
        </p>
      </div>
      {!preferences.loaded && preferences.pending === "load" && (
        <p role="status">Loading community preferences…</p>
      )}
      {preferences.error && (
        <div role="alert" className="form-error">
          <p>{preferences.error}</p>
          <button
            className="button"
            type="button"
            disabled={busy}
            onClick={preferences.reload}
          >
            Reload community preferences
          </button>
        </div>
      )}
      {preferences.loaded && (
        <form onSubmit={submit}>
          <fieldset disabled={busy}>
            <div className="setting-row">
              <div>
                <h3>
                  <label htmlFor={`${id}-playback`}>Playback reporting</label>
                </h3>
                <p>
                  Allow successful playback of supported public sources to
                  inform community rankings. Audio, paths, titles, credentials,
                  and stream URLs stay private.
                </p>
              </div>
              <input
                id={`${id}-playback`}
                type="checkbox"
                checked={preferences.playbackReporting}
                onChange={(event) =>
                  preferences.edit("playback_reporting", event.target.checked)
                }
              />
            </div>
            <div className="setting-row">
              <div>
                <h3>
                  <label htmlFor={`${id}-corrections`}>
                    Metadata corrections
                  </label>
                </h3>
                <p>
                  Allow reviewed public matching corrections to be shared.
                  Correction submission is not available yet.
                </p>
              </div>
              <input
                id={`${id}-corrections`}
                type="checkbox"
                checked={preferences.metadataCorrections}
                onChange={(event) =>
                  preferences.edit("metadata_corrections", event.target.checked)
                }
              />
            </div>
            <button className="button" disabled={!preferences.dirty}>
              Save community preferences
            </button>
            <button
              className="button"
              type="button"
              disabled={!preferences.dirty}
              onClick={preferences.cancel}
            >
              Cancel changes
            </button>
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
        <div className="setting-row">
          <div>
            <h3>Community connection recovery</h3>
            <p>
              Clear your account's saved community credentials and cancel unsent
              reports. Your privacy choices and accepted contributions are kept.
            </p>
            <button
              className="button"
              type="button"
              disabled={busy}
              onClick={() => void credentials.reset()}
            >
              Reset community credentials
            </button>
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
          </div>
        </div>
      )}
    </section>
  );
}
