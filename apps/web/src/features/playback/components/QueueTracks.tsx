import { memo } from "react";
import { usePlayer } from "./PlaybackProvider";
import Artwork from "../../artwork/components/Artwork";
import { time } from "../../../shared/format";
import { useQueuePage } from "../hooks/useQueuePage";

const QueueTracks = memo(function QueueTracks() {
  const player = usePlayer();
  const { view, page, error, loading, retry } = useQueuePage(player);
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
    return <p className="queue-empty">You’ve reached the end of your queue.</p>;
  return (
    <ol className="queue-tracks">
      {tracks.map(({ id, recording: track }) => (
        <li key={id} className={id === view.currentEntry ? "current" : ""}>
          <button
            className="queue-track"
            onClick={() => void player.selectEntry(id)}
          >
            <Artwork
              release={track.release_id ?? undefined}
              recording={track.id}
            />
            <span>
              <strong>{track.title}</strong>
              <small>{track.artist}</small>
            </span>
          </button>
          <span className="queue-duration">
            {time(track.duration_ms / 1000)}
          </span>
        </li>
      ))}
    </ol>
  );
});
export default QueueTracks;
