import type { ArtistPageProps } from "../shared/types";
import { useMutation } from "../shared/hooks/useMutation";
import Artwork from "../components/Artwork";
import AlbumGrid from "../shared/ui/AlbumGrid";
import EmptyState from "../shared/ui/EmptyState";
import Pagination from "../shared/ui/Pagination";
import TrackTable, { type TrackTableRow } from "../shared/ui/TrackTable";
import TrackToolbar from "../shared/ui/TrackToolbar";

export default function ArtistPage({
  user,
  playlists,
  detail,
}: ArtistPageProps) {
  const { artist, releases, recordings } = detail;
  const { mutate } = useMutation(user.id);
  const tracks = recordings.items.map((recording) => ({
    ...recording,
    row_id: recording.id,
  }));
  const path = `/artists/${encodeURIComponent(artist.id)}`;
  return (
    <>
      <section className="collection-hero artist-hero">
        <Artwork recording={artist.artwork_recording_id ?? undefined} />
        <div>
          <span>From your collection</span>
          <h1>{artist.name}</h1>
          <p>
            {artist.recording_count > 0 || artist.release_count === 0 ? (
              <>
                {artist.recording_count}{" "}
                {artist.recording_count === 1 ? "track" : "tracks"}
                {artist.release_count > 0 && " · "}
              </>
            ) : null}
            {artist.release_count > 0 && (
              <>
                {artist.release_count}{" "}
                {artist.release_count === 1 ? "album" : "albums"}
              </>
            )}
          </p>
        </div>
      </section>
      {releases.total > 0 && (
        <section aria-labelledby="artist-albums-title">
          <h2 className="discography-heading" id="artist-albums-title">
            Albums & appearances
          </h2>
          {releases.items.length ? (
            <AlbumGrid releases={releases.items} />
          ) : (
            <EmptyState
              title="No albums on this page"
              text="Return to the previous page to browse this artist's collection."
              libraryLink={false}
            />
          )}
          <Pagination
            page={releases}
            query=""
            path={path}
            label="Album pages"
          />
        </section>
      )}
      {recordings.total > 0 && (
        <section aria-labelledby="artist-tracks-title">
          <h2 className="discography-heading" id="artist-tracks-title">
            Tracks
          </h2>
          {tracks.length ? (
            <>
              <TrackToolbar
                tracks={tracks}
                label={
                  recordings.total > recordings.limit ? "this page" : "tracks"
                }
                description="Recording credits in your library"
              />
              <TrackTable
                rows={tracks.map<TrackTableRow>((track) => ({
                  kind: "recording",
                  track,
                }))}
                offset={recordings.offset}
                playlists={playlists}
                mutate={mutate}
              />
            </>
          ) : (
            <EmptyState
              title="No tracks on this page"
              text="Return to the previous page to listen to this artist."
              libraryLink={false}
            />
          )}
          <Pagination
            page={recordings}
            query=""
            path={path}
            parameter="recording_offset"
            label="Track pages"
          />
        </section>
      )}
    </>
  );
}
