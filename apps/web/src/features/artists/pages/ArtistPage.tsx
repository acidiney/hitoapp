import type { Schema } from "../../../shared/schema";

import { useMutation } from "../../../shared/hooks/useMutation";
import ArtistCreditIdentity from "../components/ArtistCreditIdentity";
import Artwork from "../../artwork/components/Artwork";
import CollectionHeader from "../../../shared/ui/CollectionHeader";
import Breadcrumbs from "../../../shared/ui/Breadcrumbs";
import { tintFor } from "../../../shared/ui/collectionTint";
import PlayShuffle from "../../playback/components/PlayShuffle";
import AlbumGrid from "../../catalog/components/AlbumGrid";
import EmptyState from "../../../shared/ui/EmptyState";
import Pagination from "../../../shared/ui/Pagination";
import TrackTable, {
  type TrackTableRow,
} from "../../playback/components/TrackTable";
import { artistTracksListing } from "../api/trackListings";
import TrackToolbar from "../../playback/components/TrackToolbar";

export default function ArtistPage({
  user,
  playlists,
  detail,
}: Schema<"ArtistPageProps">) {
  const { artist, releases, recordings } = detail;
  const { mutate } = useMutation(user.id);
  const tracks = recordings.items.map((recording) => ({
    ...recording,
    row_id: recording.id,
  }));
  const path = `/artists/${encodeURIComponent(artist.id)}`;
  const listing = artistTracksListing(user.id, artist.id, recordings);
  return (
    <>
      <CollectionHeader
        tint={tintFor(artist.id)}
        cover={
          <Artwork
            recording={artist.artwork_recording_id ?? undefined}
            fallback={{ id: artist.id, title: artist.name }}
          />
        }
        size="artist"
        crumbs={
          <Breadcrumbs
            crumbs={[
              { label: "Library", href: "/library" },
              { label: "Artists", href: "/artists" },
              { label: artist.name },
            ]}
          />
        }
        eyebrow="Artist · credit in your library"
        title={artist.name}
        stats={
          <>
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
          </>
        }
        actions={
          tracks.length > 0 ? (
            <PlayShuffle tracks={tracks} listing={listing} />
          ) : undefined
        }
      />
      <ArtistCreditIdentity account={user.id} artist={artist} />
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
                listing={listing}
                withPlay={false}
                description="Recording credits in your library"
              />
              <TrackTable
                rows={tracks.map<TrackTableRow>((track) => ({
                  kind: "recording",
                  track,
                }))}
                listing={listing}
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
