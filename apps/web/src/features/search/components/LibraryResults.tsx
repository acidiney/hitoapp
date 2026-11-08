import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";

import { time } from "../../../shared/format";
import type { Mutate } from "../../../shared/hooks/useMutation";
import Artwork from "../../artwork/components/Artwork";
import { releaseCreditLabel } from "../../catalog/model/albumCredit";
import TrackTable, {
  type TrackTableRow,
} from "../../playback/components/TrackTable";
import type { ArtistHit } from "../model/artistHits";

export function resultCount(shown: number, total: number): string {
  if (total > shown) return `Showing ${shown} of ${total}`;
  return shown === 1 ? "1 result" : `${shown} results`;
}

function GroupHead({
  id,
  title,
  shown,
  total,
}: {
  id: string;
  title: string;
  shown: number;
  total: number;
}) {
  return (
    <div className="search-group-head">
      <h2 id={id}>{title}</h2>
      <span className="muted">{resultCount(shown, total)}</span>
    </div>
  );
}

export function TrackResults({
  tracks,
  total,
  playlists,
  mutate,
}: {
  tracks: Schema<"Recording">[];
  total: number;
  playlists: Schema<"Playlist">[];
  mutate: Mutate;
}) {
  if (tracks.length === 0) return null;
  return (
    <section className="search-group" aria-labelledby="search-tracks">
      <GroupHead
        id="search-tracks"
        title="Tracks"
        shown={tracks.length}
        total={total}
      />
      <TrackTable
        rows={tracks.map<TrackTableRow>((track) => ({
          kind: "recording",
          track: { ...track, row_id: track.id },
        }))}
        playlists={playlists}
        mutate={mutate}
      />
    </section>
  );
}

export function AlbumResults({
  releases,
  total,
}: {
  releases: Schema<"Release">[];
  total: number;
}) {
  if (releases.length === 0) return null;
  return (
    <section className="search-group" aria-labelledby="search-albums">
      <GroupHead
        id="search-albums"
        title="Albums"
        shown={releases.length}
        total={total}
      />
      <div className="search-cards">
        {releases.map((release) => (
          <Link
            key={release.id}
            className="search-card"
            href={`/albums/${release.id}`}
          >
            <Artwork
              release={release.id}
              fallback={{
                id: release.id,
                title: release.title,
                artist: release.artist,
              }}
            />
            <span className="search-card-copy">
              <strong>{release.title}</strong>
              <span>
                {releaseCreditLabel(release) ?? "Unknown artist"} ·{" "}
                {release.track_count}{" "}
                {release.track_count === 1 ? "track" : "tracks"}
                {release.duration_ms > 0 &&
                  ` · ${time(release.duration_ms / 1000)}`}
              </span>
            </span>
          </Link>
        ))}
      </div>
    </section>
  );
}

export function ArtistResults({ artists }: { artists: ArtistHit[] }) {
  if (artists.length === 0) return null;
  return (
    <section className="search-group" aria-labelledby="search-artists">
      <GroupHead
        id="search-artists"
        title="Artists"
        shown={artists.length}
        total={artists.length}
      />
      <div className="search-cards">
        {artists.map((artist) => (
          <Link key={artist.key} className="search-card" href={artist.href}>
            <span className="search-card-copy">
              <strong>{artist.name}</strong>
              <span>
                {artist.albums} {artist.albums === 1 ? "album" : "albums"} ·{" "}
                {artist.tracks} {artist.tracks === 1 ? "track" : "tracks"}
              </span>
            </span>
          </Link>
        ))}
      </div>
    </section>
  );
}
