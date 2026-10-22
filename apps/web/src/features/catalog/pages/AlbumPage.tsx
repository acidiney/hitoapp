import { Link } from "@inertiajs/react";
import type { AlbumPageProps } from "../../../shared/types";
import { useMutation } from "../../../shared/hooks/useMutation";
import { time } from "../../../shared/format";
import Artwork from "../../artwork/components/Artwork";
import ArtworkStatus from "../../artwork/components/ArtworkStatus";
import TrackTable, {
  type TrackTableRow,
} from "../../playback/components/TrackTable";
import TrackToolbar from "../../playback/components/TrackToolbar";
import Pagination from "../../../shared/ui/Pagination";
import EmptyState from "../../../shared/ui/EmptyState";

export default function AlbumPage({ user, playlists, detail }: AlbumPageProps) {
  const { release, tracks: page } = detail;
  const { mutate } = useMutation(user.id);
  const tracks = page.items.map((track) => ({
    ...track,
    row_id: track.entry_id,
  }));
  return (
    <>
      <section className="collection-hero album-hero">
        <Artwork release={release.id} />
        <div>
          <span>Album</span>
          <h1>{release.title}</h1>
          {release.artist !== null && release.artist_id !== null ? (
            <Link
              className="credit-link"
              href={`/artists/${encodeURIComponent(release.artist_id)}`}
            >
              {release.artist}
            </Link>
          ) : (
            <span className="muted">Album artist not provided</span>
          )}
          <p>
            {release.track_count}{" "}
            {release.track_count === 1 ? "track" : "tracks"} ·{" "}
            {time(release.duration_ms / 1000)}
          </p>
          <Link
            className="text-link"
            href={`/albums/${encodeURIComponent(release.id)}/details`}
          >
            Edit album details
          </Link>
        </div>
      </section>
      <ArtworkStatus release={release.id} />
      {tracks.length ? (
        <>
          <TrackToolbar
            tracks={tracks}
            label={page.total > page.limit ? "this page" : "album"}
            description={
              page.items.some((track) => track.track_number != null)
                ? "Ordered by disc and track tags"
                : "Track numbers unavailable"
            }
          />
          <TrackTable
            rows={tracks.map<TrackTableRow>((track) => ({
              kind: "recording",
              track,
            }))}
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
      <Pagination
        page={page}
        query=""
        path={`/albums/${encodeURIComponent(release.id)}`}
      />
    </>
  );
}
