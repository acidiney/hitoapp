import { Link } from "@inertiajs/react";
import type { AlbumPageProps } from "../types";
import { useMutation } from "../hooks/useMutation";
import { time } from "../format";
import Artwork from "../components/Artwork";
import TrackTable from "../components/TrackTable";
import TrackToolbar from "../components/TrackToolbar";
import Pagination from "../components/Pagination";
import EmptyState from "../components/EmptyState";

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
          <Link
            className="credit-link"
            href={`/artists/${encodeURIComponent(release.artist_id)}`}
          >
            {release.artist}
          </Link>
          <p>
            {release.track_count}{" "}
            {release.track_count === 1 ? "track" : "tracks"} ·{" "}
            {time(release.duration_ms / 1000)}
          </p>
        </div>
      </section>
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
            tracks={tracks}
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
