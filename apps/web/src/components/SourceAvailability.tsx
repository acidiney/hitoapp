import { Link } from "@inertiajs/react";
import { Play, RefreshCw } from "lucide-react";
import type { Track } from "../playback";
import { usePlayer } from "../player/PlaybackProvider";
import { useSourceAvailability } from "../hooks/useSourceAvailability";

export default function SourceAvailability({
  account,
  track,
}: {
  account: string;
  track: Track;
}) {
  const player = usePlayer();
  const { sources, pending, error, check } = useSourceAvailability(
    account,
    track.id,
  );
  const ready = sources?.some((source) => source.ready) ?? false;
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
          <Link className="text-link" href="/settings?tab=plugins">
            Music service settings
          </Link>
        )}
      </div>
    </section>
  );
}
