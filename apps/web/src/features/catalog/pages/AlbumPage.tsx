import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";

import { useMutation } from "../../../shared/hooks/useMutation";
import { time } from "../../../shared/format";
import Breadcrumbs from "../../../shared/ui/Breadcrumbs";
import PlayShuffle from "../../playback/components/PlayShuffle";
import Artwork from "../../artwork/components/Artwork";
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
  }));
  const credit = releaseCreditLabel(
    release,
    tracks.map((track) => track.artist),
  );
  const listing = releaseTracksListing(user.id, release.id, page);
  return (
    <>
      <AlbumHeader
        crumbs={
          <Breadcrumbs
            crumbs={[
              { label: "Library", href: "/library" },
              { label: "Albums", href: "/albums" },
              { label: release.title },
            ]}
          />
        }
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
          release.artist !== null && release.artist_id !== null ? (
            <Link
              className="album-artist"
              href={`/artists/${encodeURIComponent(release.artist_id)}`}
            >
              {release.artist}
            </Link>
          ) : (
            credit && <span className="album-artist">{credit}</span>
          )
        }
        stats={
          <>
            {release.track_count}{" "}
            {release.track_count === 1 ? "track" : "tracks"} ·{" "}
            {time(release.duration_ms / 1000)}
          </>
        }
        actions={
          <>
            <PlayShuffle
              tracks={tracks}
              listing={listing}
              label="album"
              tone="plain"
            />
            <Link
              className="button"
              href={`/albums/${encodeURIComponent(release.id)}/details`}
            >
              Edit album details
            </Link>
          </>
        }
      />
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
    </>
  );
}
