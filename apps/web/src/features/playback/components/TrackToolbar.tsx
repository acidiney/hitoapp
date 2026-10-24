import type { ReactNode } from "react";
import { Play } from "lucide-react";
import type { Track } from "../model";
import type { TrackListing } from "../api/trackListing";
import { usePlayTracks } from "../hooks/usePlayTracks";

export default function TrackToolbar({
  tracks,
  listing,
  label = "tracks",
  description,
  children,
}: {
  tracks: Track[];
  listing?: TrackListing;
  label?: string;
  description: string;
  children?: ReactNode;
}) {
  const play = usePlayTracks(tracks, listing);
  return (
    <div className="list-toolbar">
      <button
        className="button primary"
        disabled={tracks.length === 0}
        onClick={() => void play.playAll()}
      >
        <Play size={17} />
        Play {label}
      </button>
      <span className="muted">{description}</span>
      {children}
    </div>
  );
}
