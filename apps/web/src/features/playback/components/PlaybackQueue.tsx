import { memo } from "react";
import { X } from "lucide-react";
import QueueTracks from "./QueueTracks";

const PlaybackQueue = memo(function PlaybackQueue({
  close,
}: {
  close: () => void;
}) {
  return (
    <aside className="queue-panel" aria-label="Playback queue">
      <div className="section-heading">
        <h2>Up next</h2>
        <button
          className="icon-button"
          aria-label="Close queue"
          onClick={close}
        >
          <X size={20} />
        </button>
      </div>
      <QueueTracks />
    </aside>
  );
});

export default PlaybackQueue;
