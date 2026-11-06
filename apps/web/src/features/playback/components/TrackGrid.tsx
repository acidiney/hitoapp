import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";
import { Play } from "lucide-react";
import type { Mutate } from "../../../shared/hooks/useMutation";

import type { TrackRow } from "./TrackTable";
import type { TrackListing } from "../api/trackListing";
import { usePlayTracks } from "../hooks/usePlayTracks";
import Artwork from "../../artwork/components/Artwork";
import TrackRowActions from "./TrackRowActions";
import TrackAvailabilityNote from "./TrackAvailabilityNote";
import TrackNames, { albumHref, artistHref } from "./TrackNames";
import { trackHref } from "./TrackRowMenu";

export default function TrackGrid({
  tracks,
  listing,
  playlists,
  mutate,
}: {
  tracks: TrackRow[];
  listing?: TrackListing;
  playlists: Schema<"Playlist">[];
  mutate: Mutate;
}) {
  const play = usePlayTracks(tracks, listing);
  return (
    <div className="track-grid">
      {tracks.map((track, index) => (
        <article className="track-card" key={track.row_id}>
          <div className="track-card-cover">
            <Artwork
              recording={track.id}
              fallback={{
                id: track.release_id ?? track.id,
                title: track.album ?? track.title,
                artist: track.artist,
              }}
            />
            <button
              className="card-play"
              aria-label={`Play ${track.title}`}
              onClick={() => void play.playFrom(index)}
            >
              <Play size={20} fill="currentColor" aria-hidden="true" />
            </button>
          </div>
          <div className="track-card-copy">
            <h3>
              <Link className="track-title" href={trackHref(track.id)}>
                {track.title}
              </Link>
            </h3>
            <p>
              <TrackNames
                artist={track.artist}
                artistHref={artistHref(track)}
                album={track.album}
                albumHref={albumHref(track)}
              />
            </p>
            <TrackAvailabilityNote track={track} />
          </div>
          <TrackRowActions
            track={track}
            playlists={playlists}
            mutate={mutate}
          />
        </article>
      ))}
    </div>
  );
}
