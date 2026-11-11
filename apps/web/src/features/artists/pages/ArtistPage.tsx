import type { Schema } from "../../../shared/schema";

import { useMutation } from "../../../shared/hooks/useMutation";
import { artistCounts } from "../model/artistCounts";
import ArtistAppearances from "../components/ArtistAppearances";
import ArtistCreditIdentity from "../components/ArtistCreditIdentity";
import Artwork from "../../artwork/components/Artwork";
import ArtistPhoto from "../components/ArtistPhoto";
import ArtistHeader from "../components/ArtistHeader";
import ArtistActions from "../components/ArtistActions";
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
  const { artist, releases, recordings, featured_recordings } = detail;
  const { mutate } = useMutation(user.id);
  const tracks = recordings.items.map((recording) => ({
    ...recording,
    row_id: recording.id,
  }));
  const path = `/artists/${encodeURIComponent(artist.id)}`;
  const listing = artistTracksListing(user.id, artist.id, recordings);
  return (
    <div className="detail-page">
      <ArtistHeader
        variant="credit"
        photo={
          <ArtistPhoto
            kind="credited-artist"
            artist={artist.id}
            identity={artist.id}
            name={artist.name}
            enabled={artist.provider_photo === true}
          >
            <Artwork
              recording={artist.artwork_recording_id ?? undefined}
              fallback={{ id: artist.id, title: artist.name, kind: "artist" }}
            />
          </ArtistPhoto>
        }
        name={artist.name}
        stats={artistCounts(
          artist.recording_count,
          artist.release_count,
          artist.featured_recording_count ?? 0,
          "credit",
        )}
        more={
          artist.member_only
            ? undefined
            : {
                label: "More actions, including Choose the artist",
                items: [
                  {
                    label: "Choose the artist",
                    onSelect: () =>
                      document.getElementById("choose-artist")?.focus(),
                  },
                ],
              }
        }
        actions={
          tracks.length > 0 ? (
            <ArtistActions
              account={user.id}
              tracks={tracks}
              listing={listing}
            />
          ) : undefined
        }
      />
      <div className="detail-body">
        {!artist.member_only && (
          <ArtistCreditIdentity
            account={user.id}
            artist={artist}
            searchRecording={
              tracks[0]?.id ?? artist.artwork_recording_id ?? undefined
            }
          />
        )}
        {recordings.total > 0 && (
          <section
            className="detail-section"
            aria-labelledby="artist-tracks-title"
          >
            <div className="detail-section-head">
              <h2 id="artist-tracks-title">Tracks</h2>
            </div>
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
                  artistPage={artist.id}
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
        {releases.total > 0 && (
          <section
            className="detail-section"
            aria-labelledby="artist-albums-title"
          >
            <div className="detail-section-head">
              <h2 id="artist-albums-title">Albums in your library</h2>
            </div>
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
        <ArtistAppearances
          account={user.id}
          artist={artist.id}
          appearances={featured_recordings}
          playlists={playlists}
          mutate={mutate}
        />
      </div>
    </div>
  );
}
