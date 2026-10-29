import type { ReactNode } from "react";
import { Play, Shuffle } from "lucide-react";
import { usePlayer } from "./PlaybackProvider";
import type { Track } from "../model";
import type { TrackListing } from "../api/trackListing";
import { usePlayTracks } from "../hooks/usePlayTracks";

export default function TrackToolbar({
  tracks,
  listing,
  label = "tracks",
  description,
  withPlay = true,
  children,
}: {
  tracks: Track[];
  listing?: TrackListing;
  label?: string;
  description?: string;
  withPlay?: boolean;
  children?: ReactNode;
}) {
  const player = usePlayer();
  const play = usePlayTracks(tracks, listing);
  async function shuffle() {
    await play.playAll();
    if (!player.snapshot().shuffle) player.shuffle();
  }
  return (
    <div className="list-toolbar">
      {withPlay && (
        <button
          className="button primary"
          disabled={tracks.length === 0}
          onClick={() => void play.playAll()}
        >
          <Play size={17} />
          Play {label}
        </button>
      )}
      {withPlay && (
        <button
          className="button"
          disabled={tracks.length === 0}
          aria-label={`Shuffle ${label}`}
          onClick={() => void shuffle()}
        >
          <Shuffle size={17} />
          Shuffle
        </button>
      )}
      {description && <span className="toolbar-note">{description}</span>}
      {children}
    </div>
  );
}
