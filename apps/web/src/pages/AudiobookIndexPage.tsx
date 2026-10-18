import { Link } from "@inertiajs/react";
import Artwork from "../components/Artwork";
import EmptyState from "../components/EmptyState";
import PageHeading from "../components/PageHeading";
import type { AudiobookIndexPageProps } from "../types";

export default function AudiobookIndexPage({ books }: AudiobookIndexPageProps) {
  return (
    <>
      <PageHeading
        title="Audiobooks"
        description="Your playlists grouped into ordered chapters."
      >
        <Link className="button" href="/playlists">
          Browse playlists
        </Link>
      </PageHeading>
      <div className="playlist-list">
        {books.map((book) => (
          <Link
            href={`/audiobooks/${encodeURIComponent(book.id)}`}
            key={book.id}
          >
            <Artwork playlist />
            <span>
              <strong>{book.name}</strong>
            </span>
            <span aria-hidden="true">→</span>
          </Link>
        ))}
      </div>
      {books.length === 0 && (
        <EmptyState
          title="No audiobooks yet"
          text="Your grouped playlists will appear here."
          libraryLink={false}
        />
      )}
    </>
  );
}
