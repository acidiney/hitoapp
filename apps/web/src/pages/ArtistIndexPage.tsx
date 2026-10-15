import { Link } from "@inertiajs/react";
import type { ArtistIndexPageProps } from "../types";
import Artwork from "../components/Artwork";
import PageHeading from "../components/PageHeading";
import EmptyState from "../components/EmptyState";
import Pagination from "../components/Pagination";

export default function ArtistIndexPage({
  artists,
  query,
}: ArtistIndexPageProps) {
  return (
    <>
      <PageHeading
        title="Artists"
        description={`${artists.total} ${artists.total === 1 ? "artist credit" : "artist credits"} in your libraries`}
      />
      {artists.items.length ? (
        <div className="artist-grid">
          {artists.items.map((artist) => (
            <Link
              className="artist-card"
              key={artist.id}
              href={`/artists/${encodeURIComponent(artist.id)}`}
            >
              <Artwork recording={artist.artwork_recording_id ?? undefined} />
              <h2>{artist.name}</h2>
              <small>
                {artist.release_count}{" "}
                {artist.release_count === 1 ? "album" : "albums"}
              </small>
            </Link>
          ))}
        </div>
      ) : (
        <EmptyState
          title={query ? "No matching artists" : "Meet your collection"}
          text={
            query
              ? "Try another artist name."
              : "Artists appear here when Hito reads the credits in your music files."
          }
          libraryLink
        />
      )}
      <Pagination page={artists} query={query} path="/artists" />
    </>
  );
}
