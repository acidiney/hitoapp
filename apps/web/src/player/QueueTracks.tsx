import { memo } from "react";
import { ChevronLeft, ChevronRight, X } from "lucide-react";
import { usePlayer } from "./PlaybackProvider";
import Artwork from "../features/artwork/components/Artwork";
import { time } from "../shared/format";
import { useQueuePage } from "./useQueuePage";

const QueueTracks = memo(function QueueTracks({
  preview = false,
}: {
  preview?: boolean;
}) {
  const player = usePlayer();
  const { view, offset, limit, page, error, loading, previous, next, retry } =
    useQueuePage(player, preview);
  const tracks = page?.entries ?? [];
  if (loading) return <p className="queue-empty">Loading your queue…</p>;
  if (error)
    return (
      <div className="queue-empty" role="alert">
        <p>{error}</p>
        <button className="text-link" onClick={retry}>
          Retry queue
        </button>
      </div>
    );
  if (!tracks.length)
    return (
      <p className="queue-empty">
        {preview
          ? "You’ve reached the end of your queue."
          : "Choose a track to start listening. Your queue stays with this device."}
      </p>
    );
  return (
    <>
      <ol className="queue-tracks" start={offset + 1}>
        {tracks.map(({ id, recording: track }) => {
          return (
            <li key={id} className={id === view.currentEntry ? "current" : ""}>
              <button
                className="queue-track"
                onClick={() => void player.selectEntry(id)}
              >
                <Artwork recording={track.id} />
                <span>
                  <strong>{track.title}</strong>
                  <small>{track.artist}</small>
                </span>
              </button>
              <span className="queue-duration">
                {time(track.duration_ms / 1000)}
              </span>
              {!preview && (
                <button
                  className="icon-button"
                  aria-label={`Remove ${track.title} from queue`}
                  onClick={() => player.removeEntry(id)}
                >
                  <X size={16} />
                </button>
              )}
            </li>
          );
        })}
      </ol>
      {!preview && view.total > limit && (
        <nav className="pagination queue-pages" aria-label="Queue pages">
          <button
            className="icon-button"
            disabled={offset === 0}
            onClick={previous}
            aria-label="Previous queue page"
          >
            <ChevronLeft size={18} />
          </button>
          <span>
            {offset + 1} to {Math.min(offset + limit, view.total)} of{" "}
            {view.total}
          </span>
          <button
            className="icon-button"
            disabled={offset + limit >= view.total}
            onClick={next}
            aria-label="Next queue page"
          >
            <ChevronRight size={18} />
          </button>
        </nav>
      )}
    </>
  );
});
export default QueueTracks;
