import { Link } from "@inertiajs/react";
import { Play } from "lucide-react";
import type { Artist, Recording, Release } from "../../../shared/types";
import { time } from "../../../shared/format";
import Artwork from "../../artwork/components/Artwork";
import AvailabilityLabel from "../../../shared/ui/AvailabilityLabel";
import Button from "../../../shared/ui/Button";
import ListRow from "../../../shared/ui/ListRow";

function trackCover(track: Recording) {
  return {
    id: track.release_id ?? track.id,
    title: track.album ?? track.title,
    artist: track.artist,
  };
}

function TrackRow({
  track,
  onPlay,
}: {
  track: Recording;
  onPlay: (track: Recording) => void;
}) {
  return (
    <ListRow
      cover={<Artwork recording={track.id} fallback={trackCover(track)} />}
      title={track.title}
      subtitle={[track.artist, track.album].filter(Boolean).join(" · ")}
      state={
        <AvailabilityLabel state={track.available ? "library" : "catalog"} />
      }
      action={
        track.available ? (
          <Button
            aria-label={`Play ${track.title}`}
            onClick={() => onPlay(track)}
          >
            <Play size={14} fill="currentColor" aria-hidden="true" />
            Play
          </Button>
        ) : (
          <Link
            className="button secondary"
            href={`/recordings/${encodeURIComponent(track.id)}`}
            aria-label={`View ${track.title}`}
          >
            View
          </Link>
        )
      }
    />
  );
}

export function TopResult({
  track,
  release,
  artist,
}: {
  track?: Recording;
  release?: Release;
  artist?: Artist;
}) {
  if (!track && !release && !artist) return null;
  return (
    <section className="search-group search-top" aria-labelledby="top-result">
      <h2 id="top-result">Top result</h2>
      {track ? (
        <Link
          className="search-card"
          href={`/recordings/${encodeURIComponent(track.id)}`}
        >
          <Artwork recording={track.id} fallback={trackCover(track)} />
          <span className="search-card-copy">
            <strong>{track.title}</strong>
            <span>Track · {track.artist}</span>
          </span>
        </Link>
      ) : release ? (
        <Link className="search-card" href={`/albums/${release.id}`}>
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
            <span>Album · {release.artist ?? "Unknown artist"}</span>
          </span>
        </Link>
      ) : artist ? (
        <Link className="search-card" href={`/artists/${artist.id}`}>
          <span className="search-card-copy">
            <strong>{artist.name}</strong>
            <span>Artist</span>
          </span>
        </Link>
      ) : null}
    </section>
  );
}

export function TrackResults({
  tracks,
  onPlay,
}: {
  tracks: Recording[];
  onPlay: (track: Recording) => void;
}) {
  if (tracks.length === 0) return null;
  return (
    <section className="search-group" aria-labelledby="search-tracks">
      <h2 id="search-tracks">Tracks</h2>
      {tracks.map((track) => (
        <TrackRow key={track.id} track={track} onPlay={onPlay} />
      ))}
    </section>
  );
}

export function AlbumResults({ releases }: { releases: Release[] }) {
  if (releases.length === 0) return null;
  return (
    <section className="search-group" aria-labelledby="search-albums">
      <h2 id="search-albums">Albums</h2>
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
                {release.artist ?? "Unknown artist"} · {release.track_count}{" "}
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

export function ArtistResults({ artists }: { artists: Artist[] }) {
  if (artists.length === 0) return null;
  return (
    <section className="search-group" aria-labelledby="search-artists">
      <h2 id="search-artists">Artists</h2>
      <div className="search-cards">
        {artists.map((artist) => (
          <Link
            key={artist.id}
            className="search-card"
            href={`/artists/${artist.id}`}
          >
            <span className="search-card-copy">
              <strong>{artist.name}</strong>
              <span>
                {artist.release_count}{" "}
                {artist.release_count === 1 ? "album" : "albums"} ·{" "}
                {artist.recording_count}{" "}
                {artist.recording_count === 1 ? "track" : "tracks"}
              </span>
            </span>
          </Link>
        ))}
      </div>
    </section>
  );
}
