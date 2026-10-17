import { Link } from "@inertiajs/react";
import { Play, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import type { Track } from "../playback";
import { usePlayer } from "../player/PlaybackProvider";
import { useSourceAvailability } from "../hooks/useSourceAvailability";
import { useSourcePreparation } from "../hooks/useSourcePreparation";
import SourcePreparation from "./SourcePreparation";

export default function SourceAvailability({
  account,
  track,
}: {
  account: string;
  track: Track;
}) {
  const player = usePlayer();
  const { sources, pending, error, check, confirmReady, revision } =
    useSourceAvailability(account, track.id);
  const key = JSON.stringify([account, track.id]);
  const [selection, setSelection] = useState<{
    key: string;
    id: string;
  } | null>(null);
  const selected =
    sources?.find(
      (source) => selection?.key === key && source.id === selection.id,
    ) ??
    sources?.find((source) => source.can_prepare) ??
    null;
  const selectedId = selected?.id;
  useEffect(() => {
    if (selectedId) setSelection({ key, id: selectedId });
    else setSelection(null);
  }, [key, selectedId]);
  const preparation = useSourcePreparation(
    account,
    track.id,
    selected,
    confirmReady,
    revision,
  );
  const ready =
    sources?.some(
      (source) => source.recording_id === track.id && source.ready === true,
    ) ?? false;
  return (
    <section
      className="source-availability"
      aria-labelledby="audio-availability-heading"
    >
      <h2 id="audio-availability-heading">Audio availability</h2>
      <p>Check your music service for audio that is ready to play.</p>
      <div role="status">
        {pending
          ? "Checking audio…"
          : sources !== null
            ? ready
              ? "Audio is ready to play."
              : "No audio is ready yet."
            : null}
      </div>
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      {sources && sources.length > 0 && (
        <ul>
          {sources.map((source) => (
            <li key={source.id}>
              {source.can_prepare || source.id === selectedId ? (
                <label>
                  <input
                    type="radio"
                    name="preparation-source"
                    checked={source.id === selectedId}
                    onChange={() => setSelection({ key, id: source.id })}
                  />{" "}
                  {source.label}
                </label>
              ) : (
                source.label
              )}
            </li>
          ))}
        </ul>
      )}
      {selected && (
        <SourcePreparation
          {...preparation}
          retrying={preparation.attempt !== null}
        />
      )}
      <div className="source-availability-actions">
        <button
          className="button"
          disabled={pending}
          onClick={() => void check()}
        >
          <RefreshCw size={16} />{" "}
          {pending
            ? "Checking…"
            : sources !== null || error
              ? "Check again"
              : "Check availability"}
        </button>
        {ready && (
          <button
            className="button primary"
            disabled={pending}
            onClick={() =>
              void player.playQueue([{ ...track, available: true }])
            }
          >
            <Play size={16} /> Play track
          </button>
        )}
        {(error || (sources !== null && !ready)) && (
          <Link className="text-link" href="/settings?section=plugins">
            Music service settings
          </Link>
        )}
      </div>
    </section>
  );
}
