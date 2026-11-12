import type { Schema } from "../../../shared/schema";
import { useMemo, useState } from "react";
import RouteLink from "../../../shared/ui/RouteLink";

import { useMutation } from "../../../shared/hooks/useMutation";
import { longDuration } from "../../artists/model/catalogAlbumRows";
import AlbumHeaderActions from "../components/AlbumHeaderActions";
import Artwork from "../../artwork/components/Artwork";
import ArtworkStatus from "../../artwork/components/ArtworkStatus";
import TrackTable, {
  type TrackTableRow,
} from "../../playback/components/TrackTable";
import TrackToolbar from "../../playback/components/TrackToolbar";
import Pagination from "../../../shared/ui/Pagination";
import EmptyState from "../../../shared/ui/EmptyState";
import { releaseCreditLabel } from "../model/albumCredit";
import { albumEyebrow } from "../model/albumEyebrow";
import { releaseTracksListing } from "../api/trackListings";
import AlbumHeader from "../components/AlbumHeader";
import AlbumCompleteTrackList from "../components/AlbumCompleteTrackList";
import {
  AlbumMergeContext,
  type AlbumProgress,
  type EditionPlay,
} from "../components/AlbumMergeContext";
import CatalogPlayNotes from "../../artists/components/CatalogPlayNotes";

function AlbumProgressLine({ progress }: { progress: AlbumProgress }) {
  const { owned, total, elsewhere } = progress;
  const percent = total === 0 ? 0 : Math.round((owned / total) * 100);
  return (
    <span className="album-status" data-tone="progress">
      <span
        className="album-progress"
        role="img"
        aria-label={`${owned} of ${total} tracks in your library`}
      >
        <span style={{ width: `${percent}%` }} />
      </span>
      <span>
        {owned} of {total} in your library
        {elsewhere > 0 ? ` · ${elsewhere} more not in this edition` : ""}
      </span>
    </span>
  );
}

export default function AlbumPage({
  user,
  playlists,
  detail,
}: Schema<"AlbumPageProps">) {
  const { release, tracks: page } = detail;
  const { mutate } = useMutation(user.id);
  const [merged, setMerged] = useState(false);
  const [progress, setProgress] = useState<AlbumProgress | null>(null);
  const [editionPlay, setEditionPlay] = useState<EditionPlay | null>(null);
  const merge = useMemo(
    () => ({
      playlists,
      mutate,
      setMerged,
      merged,
      setProgress,
      editionPlay,
      setEditionPlay,
    }),
    [playlists, mutate, merged, editionPlay],
  );
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
  const trackCount = editionPlay?.tracks ?? release.track_count;
  const durationMs = editionPlay?.durationMs ?? release.duration_ms;
  const duration = longDuration(durationMs);
  return (
    <AlbumMergeContext.Provider value={merge}>
      <div className="detail-page">
        <AlbumHeader
          eyebrow={albumEyebrow(null, release.year)}
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
          stats={`${trackCount} ${trackCount === 1 ? "track" : "tracks"}${durationMs >= 60_000 ? ` · ${duration}` : ""}`}
          status={progress && <AlbumProgressLine progress={progress} />}
          actions={
            <AlbumHeaderActions
              release={release.id}
              tracks={tracks}
              listing={listing}
            />
          }
          notes={
            editionPlay && (
              <CatalogPlayNotes
                consentOff={editionPlay.consentOff}
                halted={editionPlay.halted}
              />
            )
          }
        />
        <div className="album-body">
          <ArtworkStatus release={release.id} />
          {tracks.length && merged ? null : tracks.length ? (
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
                album={{
                  title: release.title,
                  year: release.year,
                  trackCount: release.track_count,
                }}
                anchor={detail.catalog_anchor}
                saved={detail.saved_edition}
              />
            )}
          {!merged && (
            <Pagination
              page={page}
              query=""
              path={`/albums/${encodeURIComponent(release.id)}`}
            />
          )}
        </div>
      </div>
    </AlbumMergeContext.Provider>
  );
}
