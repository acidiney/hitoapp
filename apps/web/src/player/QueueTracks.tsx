import { memo, useSyncExternalStore } from "react";
import { X } from "lucide-react";
import { usePlayer } from "./PlaybackProvider";
import Artwork from "../components/Artwork";
import { time } from "../format";

const QueueTracks = memo(function QueueTracks({
  preview = false,
}: {
  preview?: boolean;
}) {
  const player = usePlayer();
  const queue = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().queue,
  );
  const current = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().index,
  );
  const start = preview ? Math.max(0, current + 1) : 0;
  const tracks = queue.slice(start, preview ? start + 4 : undefined);
  if (!tracks.length)
    return (
      <p className="queue-empty">
        {preview
          ? "You’ve reached the end of your queue."
          : "Choose a track to start listening. Your queue stays with this browser."}
      </p>
    );
  return (
    <ol className="queue-tracks">
      {tracks.map((track, offset) => {
        const index = start + offset;
        return (
          <li
            key={`${track.id}-${index}`}
            className={index === current ? "current" : ""}
          >
            <button
              className="queue-track"
              onClick={() => void player.playQueue(queue, index)}
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
                onClick={() => player.remove(index)}
              >
                <X size={16} />
              </button>
            )}
          </li>
        );
      })}
    </ol>
  );
});
export default QueueTracks;
