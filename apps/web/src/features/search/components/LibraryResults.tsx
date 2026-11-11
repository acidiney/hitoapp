import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";

import { time } from "../../../shared/format";
import type { Mutate } from "../../../shared/hooks/useMutation";
import Artwork from "../../artwork/components/Artwork";
import FallbackCover from "../../../shared/ui/FallbackCover";
import { releaseCreditLabel } from "../../catalog/model/albumCredit";
import type { ArtistHit } from "../model/artistHits";
import TrackResultRows from "./TrackResultRows";

export function resultCount(shown: number, total: number): string {
  if (total > shown) return `Showing ${shown} of ${total}`;
  return shown === 1 ? "1 result" : `${shown} results`;
}

function GroupHead({
  id,
  title,
  more,
  note,
}: {
  id: string;
  title: string;
  more?: { label: string; onClick: () => void };
  note?: string;
}) {
  return (
    <div className="search-group-head">
      <h2 id={id}>{title}</h2>
      {note && <span className="muted">{note}</span>}
      {more && (
        <button type="button" className="text-link" onClick={more.onClick}>
          {more.label}
        </button>
      )}
    </div>
  );
}

export function TrackResults({
  tracks,
  total,
  limit,
  playlists,
  mutate,
  onShowAll,
}: {
  tracks: Schema<"Recording">[];
  total: number;
  limit: number | null;
  playlists: Schema<"Playlist">[];
  mutate: Mutate;
  onShowAll: () => void;
}) {
  if (tracks.length === 0) return null;
  const shown = limit === null ? tracks : tracks.slice(0, limit);
  return (
    <section className="search-group" aria-labelledby="search-tracks">
      <GroupHead
        id="search-tracks"
        title="Tracks"
        more={
          limit !== null && shown.length < total
            ? { label: `Show all ${total}`, onClick: onShowAll }
            : undefined
        }
        note={
          limit === null && shown.length < total
            ? resultCount(shown.length, total)
            : undefined
        }
      />
      <TrackResultRows
        tracks={shown.map((track) => ({ ...track, row_id: track.id }))}
        playlists={playlists}
        mutate={mutate}
      />
    </section>
  );
}

export function AlbumResults({
  releases,
  heading = "Albums",
}: {
  releases: Schema<"Release">[];
  heading?: string;
}) {
  if (releases.length === 0) return null;
  return (
    <section className="search-group" aria-labelledby="search-albums">
      <GroupHead id="search-albums" title={heading} />
      <div className="search-cards">
        {releases.map((release) => (
          <Link
            key={release.id}
            className="search-card"
            href={`/albums/${release.id}`}
            data-search-row=""
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

export function ArtistResults({
  artists,
  heading = "Artists",
}: {
  artists: ArtistHit[];
  heading?: string;
}) {
  if (artists.length === 0) return null;
  return (
    <section className="search-group" aria-labelledby="search-artists">
      <GroupHead id="search-artists" title={heading} />
      <div className="search-cards">
        {artists.map((artist) => (
          <Link
            key={artist.key}
            className="search-card"
            href={artist.href}
            data-search-row=""
          >
            <span
              className="search-card-art"
              data-shape="round"
              aria-hidden="true"
            >
              <FallbackCover
                kind="artist"
                id={artist.key}
                title={artist.name}
              />
            </span>
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
