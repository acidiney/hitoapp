import type { ReactNode } from "react";
import { Play, Shuffle } from "lucide-react";
import type { Track } from "../model";
import type { TrackListing } from "../api/trackListing";
import { usePlayTracks } from "../hooks/usePlayTracks";
import Button from "../../../shared/ui/Button";

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
  const play = usePlayTracks(tracks, listing);
  return (
    <div className="list-toolbar">
      {withPlay && (
        <Button
          variant="primary"
          disabled={tracks.length === 0}
          onClick={() => void play.playAll()}
        >
          <Play size={17} />
          Play {label}
        </Button>
      )}
      {withPlay && (
        <Button
          disabled={tracks.length === 0}
          aria-label={`Shuffle ${label}`}
          onClick={() => void play.shuffleAll()}
        >
          <Shuffle size={17} />
          Shuffle
        </Button>
      )}
      {description && <span className="toolbar-note">{description}</span>}
      {children}
    </div>
  );
}
