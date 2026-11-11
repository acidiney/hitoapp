import { ArrowDown, ArrowUp } from "lucide-react";
import Button from "../../../shared/ui/Button";
import SectionWarning from "../../../shared/ui/SectionWarning";
import { useReportUnsaved } from "../../../shared/unsavedChanges";
import { describeSource } from "../../sources/components/SourcePreferences";
import {
  entryKey,
  useSourcePreferences,
} from "../../sources/hooks/useSourcePreferences";

export default function PluginPlaybackOrder({ account }: { account: string }) {
  const preferences = useSourcePreferences(account);
  useReportUnsaved(preferences.dirty);
  const { entries } = preferences;
  return (
    <section
      className="plugin-order"
      aria-labelledby="plugin-order-title"
      aria-busy={preferences.pending}
    >
      <h2 id="plugin-order-title">Playback order</h2>
      <p>
        Which source plays first when a track has several. A source you choose
        for one track always wins.
      </p>
      {preferences.error && (
        <SectionWarning tone="error" title={preferences.error}>
          <Button disabled={preferences.pending} onClick={preferences.reload}>
            Reload playback order
          </Button>
        </SectionWarning>
      )}
      {preferences.loaded && (
        <>
          <ol aria-label="Source order">
            {entries.map((entry, index) => {
              const { label, hint } = describeSource(
                entry,
                preferences.connections,
              );
              return (
                <li key={entryKey(entry)}>
                  <span className="plugin-order-number">{index + 1}</span>
                  <span className="plugin-order-name">
                    <strong>{label}</strong>
                    <span>{hint}</span>
                  </span>
                  <button
                    type="button"
                    className="plugin-order-move"
                    aria-label={`Move up ${label}`}
                    disabled={preferences.pending || index === 0}
                    onClick={() => preferences.move(entry, -1)}
                  >
                    <ArrowUp size={16} aria-hidden="true" />
                  </button>
                  <button
                    type="button"
                    className="plugin-order-move"
                    aria-label={`Move down ${label}`}
                    disabled={
                      preferences.pending || index === entries.length - 1
                    }
                    onClick={() => preferences.move(entry, 1)}
                  >
                    <ArrowDown size={16} aria-hidden="true" />
                  </button>
                </li>
              );
            })}
          </ol>
          <label className="plugin-order-fallback">
            <input
              type="checkbox"
              checked={preferences.fallback}
              disabled={preferences.pending}
              onChange={(event) =>
                preferences.setFallback(event.target.checked)
              }
            />
            <span>
              <strong>Try the next source if one fails</strong>
              <span>Up to six sources are tried for one play.</span>
            </span>
          </label>
          {preferences.dirty && (
            <Button
              variant="primary"
              disabled={preferences.pending}
              onClick={() => void preferences.save()}
            >
              Save playback order
            </Button>
          )}
          {preferences.saved && <p role="status">Playback order saved.</p>}
        </>
      )}
    </section>
  );
}
