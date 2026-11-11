import { Play, Shuffle } from "lucide-react";
import type { Track } from "../../playback/model";
import type { TrackListing } from "../../playback/api/trackListing";
import { usePlayTracks } from "../../playback/hooks/usePlayTracks";

export default function AlbumPlayActions({
  tracks,
  listing,
  shuffle = true,
}: {
  tracks: Track[];
  listing?: TrackListing;
  shuffle?: boolean;
}) {
  const play = usePlayTracks(tracks, listing);
  const empty = tracks.length === 0;
  return (
    <>
      <button
        type="button"
        className="pill-action"
        data-primary=""
        aria-label="Play album"
        disabled={empty}
        onClick={() => void play.playAll()}
      >
        <Play size={16} fill="currentColor" aria-hidden="true" />
        Play
      </button>
      {shuffle && (
        <button
          type="button"
          className="pill-icon"
          aria-label="Shuffle album"
          disabled={empty}
          onClick={() => void play.shuffleAll()}
        >
          <Shuffle size={18} aria-hidden="true" />
        </button>
      )}
    </>
  );
}
