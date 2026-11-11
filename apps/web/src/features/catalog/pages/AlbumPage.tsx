import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";
import RouteLink from "../../../shared/ui/RouteLink";

import { useMutation } from "../../../shared/hooks/useMutation";
import { longDuration } from "../../artists/model/catalogAlbumRows";
import AlbumPlayActions from "../components/AlbumPlayActions";
import Artwork from "../../artwork/components/Artwork";
import ArtworkRetryMenu from "../../artwork/components/ArtworkRetryMenu";
import ArtworkStatus from "../../artwork/components/ArtworkStatus";
import TrackTable, {
  type TrackTableRow,
} from "../../playback/components/TrackTable";
import TrackToolbar from "../../playback/components/TrackToolbar";
import Pagination from "../../../shared/ui/Pagination";
import EmptyState from "../../../shared/ui/EmptyState";
import { releaseCreditLabel } from "../model/albumCredit";
import { releaseTracksListing } from "../api/trackListings";
import AlbumHeader from "../components/AlbumHeader";
import AlbumCompleteTrackList from "../components/AlbumCompleteTrackList";

export default function AlbumPage({
  user,
  playlists,
  detail,
}: Schema<"AlbumPageProps">) {
  const { release, tracks: page } = detail;
  const { mutate } = useMutation(user.id);
  const tracks = page.items.map((track) => ({
    ...track,
    row_id: track.entry_id,
    artist_id:
      release.artist_id !== null && track.artist === release.artist
        ? release.artist_id
        : null,
  }));
  const sharedArtist =
    release.artist !== null &&
    tracks.every((track) => track.artist === release.artist);
  const credit = releaseCreditLabel(
    release,
    tracks.map((track) => track.artist),
  );
  const listing = releaseTracksListing(user.id, release.id, page);
  const trackArtistIds = new Set(page.items.map((track) => track.artist_id));
  const sharedTrackArtist =
    trackArtistIds.size === 1 ? ([...trackArtistIds][0] ?? null) : null;
  const artistId = release.artist_id ?? sharedTrackArtist;
  const duration = longDuration(release.duration_ms);
  return (
    <div className="detail-page">
      <AlbumHeader
        eyebrow={["Album", release.year].filter(Boolean).join(" · ")}
        cover={
          <Artwork
            release={release.id}
            fallback={{
              id: release.id,
              title: release.title,
              artist: release.artist,
            }}
          />
        }
        title={release.title}
        credit={
          credit &&
          (artistId ? (
            <RouteLink
              className="album-artist"
              href={`/artists/${encodeURIComponent(artistId)}`}
            >
              {credit}
            </RouteLink>
          ) : (
            <span className="album-artist" data-plain="">
              {credit}
            </span>
          ))
        }
        stats={`${release.track_count} ${release.track_count === 1 ? "track" : "tracks"}${release.duration_ms >= 60_000 ? ` · ${duration}` : ""}`}
        actions={
          <>
            <AlbumPlayActions tracks={tracks} listing={listing} />
            <Link
              className="pill-action"
              href={`/albums/${encodeURIComponent(release.id)}/details`}
            >
              Edit album details
            </Link>
            <ArtworkRetryMenu release={release.id} />
          </>
        }
      />
      <div className="album-body">
        <ArtworkStatus release={release.id} />
        {tracks.length ? (
          <>
            <TrackToolbar
              tracks={tracks}
              listing={listing}
              label="album"
              withPlay={false}
            />
            <TrackTable
              rows={tracks.map<TrackTableRow>((track) => ({
                kind: "recording",
                track,
              }))}
              listing={listing}
              offset={page.offset}
              playlists={playlists}
              mutate={mutate}
              without={sharedArtist ? ["album", "artist"] : ["album"]}
            />
          </>
        ) : (
          <EmptyState
            title="No tracks on this page"
            text="Return to the previous page to listen to this album."
          />
        )}
        {detail.saved_edition &&
          (detail.catalog_anchor ||
            detail.saved_edition.state.status !== "unset") && (
            <AlbumCompleteTrackList
              key={`${user.id}/${release.id}`}
              account={user.id}
              release={release.id}
              anchor={detail.catalog_anchor}
              saved={detail.saved_edition}
            />
          )}
        <Pagination
          page={page}
          query=""
          path={`/albums/${encodeURIComponent(release.id)}`}
        />
      </div>
    </div>
  );
}
