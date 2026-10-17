import { Link } from "@inertiajs/react";
import { Play, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import type { Track } from "../playback";
import type { Source } from "../types";
import { usePlayer } from "../player/PlaybackProvider";
import { useSourceAvailability } from "../hooks/useSourceAvailability";
import { useSourcePreparation } from "../hooks/useSourcePreparation";
import SourcePreparation from "./SourcePreparation";

export default function SourceAvailability({
  account,
  track,
  choiceRevision,
}: {
  account: string;
  track: Track;
  choiceRevision?: number;
}) {
  const player = usePlayer();
  const { sources, pending, error, verified, check, confirmReady, revision } =
    useSourceAvailability(account, track.id, choiceRevision);
  const key = JSON.stringify([account, track.id]);
  const [selection, setSelection] = useState<{
    key: string;
    source: Source;
  } | null>(null);
  const remembered = selection?.key === key ? selection.source : null;
  const selected =
    sources?.find((source) => source.id === remembered?.id) ??
    remembered ??
    sources?.find((source) => source.can_prepare) ??
    null;
  const selectedId = selected?.id;
  useEffect(() => {
    if (selected)
      setSelection((current) =>
        current?.key === key && current.source === selected
          ? current
          : { key, source: selected },
      );
    else setSelection(null);
  }, [key, selected]);
  const preparationSource =
    selected && verified && sources?.some((source) => source.id === selected.id)
      ? selected
      : selected
        ? { ...selected, ready: false, can_prepare: false }
        : null;
  const preparation = useSourcePreparation(
    account,
    track.id,
    preparationSource,
    confirmReady,
    revision,
  );
  const ready =
    verified &&
    (sources?.some(
      (source) => source.recording_id === track.id && source.ready === true,
    ) ??
      false);
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
          : verified && sources !== null
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
                    disabled={!verified}
                    onChange={() => setSelection({ key, source })}
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
