import { useId, type FormEvent } from "react";
import { useCommunityRankingPreferences } from "../hooks/useCommunityRankingPreferences";

export default function CommunityRankingSettings({
  account,
}: {
  account: string;
}) {
  const preference = useCommunityRankingPreferences(account);
  const id = useId();
  const busy = preference.pending !== null;
  function submit(event: FormEvent) {
    event.preventDefault();
    void preference.save();
  }
  return (
    <section
      className="settings-panel"
      aria-label="Community source rankings"
      aria-busy={busy}
    >
      <div className="settings-intro">
        <h2>Community source rankings</h2>
        <p>
          Use community results to choose among suitable available music
          sources. Your saved sources and personal source choices keep priority.
        </p>
        <p>
          This requires a configured community service. Receiving rankings does
          not enable playback reporting. Playback continues when the community
          service is unavailable.
        </p>
      </div>
      {!preference.loaded && busy && (
        <p role="status">Loading source ranking preference…</p>
      )}
      {preference.error && (
        <div role="alert" className="form-error">
          <p>{preference.error}</p>
          <button
            className="button"
            type="button"
            disabled={busy}
            onClick={preference.reload}
          >
            Reload source ranking preference
          </button>
        </div>
      )}
      {preference.loaded && (
        <form onSubmit={submit}>
          <fieldset>
            <div className="setting-row">
              <label htmlFor={id}>Use community source rankings</label>
              <input
                id={id}
                type="checkbox"
                disabled={busy}
                checked={preference.enabled}
                onChange={(event) => preference.edit(event.target.checked)}
              />
            </div>
            <button
              className="button"
              disabled={busy || !preference.dirty || preference.stale}
            >
              Save source ranking preference
            </button>
            <button
              className="button"
              type="button"
              disabled={busy || !preference.dirty}
              onClick={preference.cancel}
            >
              Cancel changes
            </button>
          </fieldset>
          {preference.pending === "save" && (
            <p role="status">Saving source ranking preference…</p>
          )}
          {preference.saved && (
            <p role="status">Source ranking preference saved.</p>
          )}
        </form>
      )}
    </section>
  );
}
