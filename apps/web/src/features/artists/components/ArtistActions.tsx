import { Play, Shuffle } from "lucide-react";
import type { Schema } from "../../../shared/schema";
import type { Track } from "../../playback/model";
import type { TrackListing } from "../../playback/api/trackListing";
import { usePlayTracks } from "../../playback/hooks/usePlayTracks";
import type { useArtistRadio } from "../../discovery/hooks/useArtistRadio";
import {
  PHONE_QUERY,
  useMediaQuery,
} from "../../../shared/hooks/useMediaQuery";

export default function ArtistActions({
  tracks,
  listing,
  group,
  radio,
}: {
  tracks: Track[];
  listing?: TrackListing;
  group?: Schema<"ArtistIdentityGroup">;
  radio?: ReturnType<typeof useArtistRadio>;
}) {
  const play = usePlayTracks(tracks, listing);
  const phone = useMediaQuery(PHONE_QUERY);
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
            className={phone ? "pill-icon" : "pill-action"}
            aria-label="Shuffle tracks"
            onClick={() => void play.shuffleAll()}
          >
            {phone ? <Shuffle size={18} aria-hidden="true" /> : "Shuffle"}
          </button>
        </>
      )}
      {group && radio && !phone && (
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
      {radio?.notice && (
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
