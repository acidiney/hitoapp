import { Play, Shuffle } from "lucide-react";
import type { Track } from "../../playback/model";
import type { TrackListing } from "../../playback/api/trackListing";
import { usePlayTracks } from "../../playback/hooks/usePlayTracks";
import type { EditionPlay } from "./AlbumMergeContext";

export default function AlbumPlayActions({
  tracks,
  listing,
  shuffle = true,
  edition = null,
}: {
  tracks: Track[];
  listing?: TrackListing;
  shuffle?: boolean;
  edition?: EditionPlay | null;
}) {
  const play = usePlayTracks(tracks, listing);
  const empty = edition ? edition.tracks === 0 : tracks.length === 0;
  return (
    <>
      <button
        type="button"
        className="pill-action"
        data-primary=""
        aria-label="Play album"
        disabled={empty}
        onClick={() => void (edition ? edition.play() : play.playAll())}
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
          onClick={() => void (edition ? edition.shuffle() : play.shuffleAll())}
        >
          <Shuffle size={18} aria-hidden="true" />
        </button>
      )}
    </>
  );
}
