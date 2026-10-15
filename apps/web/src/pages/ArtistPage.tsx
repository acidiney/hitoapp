import type { ArtistPageProps } from "../types";
import Artwork from "../components/Artwork";
import AlbumGrid from "../components/AlbumGrid";
import EmptyState from "../components/EmptyState";
import Pagination from "../components/Pagination";

export default function ArtistPage({ detail }: ArtistPageProps) {
  const { artist, releases } = detail;
  return (
    <>
      <section className="collection-hero artist-hero">
        <Artwork recording={artist.artwork_recording_id ?? undefined} />
        <div>
          <span>From your collection</span>
          <h1>{artist.name}</h1>
          <p>
            {artist.release_count}{" "}
            {artist.release_count === 1 ? "album" : "albums"} · Artist and album
            credits
          </p>
        </div>
      </section>
      <h2 className="discography-heading">Albums & appearances</h2>
      {releases.items.length ? (
        <AlbumGrid releases={releases.items} />
      ) : (
        <EmptyState
          title="No albums on this page"
          text="Return to the previous page to browse this artist's collection."
        />
      )}
      <Pagination
        page={releases}
        query=""
        path={`/artists/${encodeURIComponent(artist.id)}`}
      />
    </>
  );
}
