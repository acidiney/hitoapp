import { Play } from "lucide-react";
import type { Schema } from "../../../shared/schema";
import { usePlayer } from "../../playback/components/PlaybackProvider";
import type { Track } from "../../playback/model";
import type { TrackListing } from "../../playback/api/trackListing";
import { usePlayTracks } from "../../playback/hooks/usePlayTracks";
import { useArtistRadio } from "../../discovery/hooks/useArtistRadio";

export default function ArtistActions({
  account,
  tracks,
  listing,
  group,
}: {
  account: string;
  tracks: Track[];
  listing?: TrackListing;
  group?: Schema<"ArtistIdentityGroup">;
}) {
  const play = usePlayTracks(tracks, listing);
  const player = usePlayer();
  const radio = useArtistRadio(account, player);
  const empty = tracks.length === 0;
  return (
    <>
      {!empty && (
        <>
          <button
            type="button"
            className="pill-action"
            data-primary=""
            aria-label="Play tracks"
            onClick={() => void play.playAll()}
          >
            <Play size={16} fill="currentColor" aria-hidden="true" />
            Play
          </button>
          <button
            type="button"
            className="pill-action"
            aria-label="Shuffle tracks"
            onClick={() => void play.shuffleAll()}
          >
            Shuffle
          </button>
        </>
      )}
      {group && (
        <button
          type="button"
          className="pill-action"
          aria-busy={radio.pending === group.id || undefined}
          disabled={radio.pending !== null}
          onClick={() => void radio.start(group)}
        >
          Start radio
        </button>
      )}
      {radio.notice && (
        <p
          className="artist-radio-notice"
          role={radio.notice.tone === "error" ? "alert" : "status"}
        >
          {radio.notice.text}
        </p>
      )}
    </>
  );
}
