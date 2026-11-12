import LibraryHeading from "../../../app/LibraryHeading";
import EmptyState from "../../../shared/ui/EmptyState";
import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";
import Artwork from "../../artwork/components/Artwork";
import AudiobookPagination from "../components/AudiobookPagination";
import AudiobookCreate from "../components/AudiobookCreate";
import { useAudiobookPageNavigation } from "../hooks/useAudiobookPageNavigation";
import { useAudiobookGrouping } from "../hooks/useAudiobookGrouping";

export default function AudiobookIndexPage({
  user,
  books,
  book_pagination,
  grouping_playlists,
}: Schema<"AudiobookIndexPageProps">) {
  const navigation = useAudiobookPageNavigation(user.id, book_pagination);
  const grouping = useAudiobookGrouping(user.id, grouping_playlists, books);
  return (
    <>
      <LibraryHeading
        path="/audiobooks"
        title="Audiobooks"
        description={`${books.length} ${books.length === 1 ? "audiobook" : "audiobooks"}${
          book_pagination.next_cursor || book_pagination.previous_cursor
            ? " on this page"
            : ""
        }`}
      >
        <div className="library-heading-actions">
          <AudiobookCreate {...grouping} />
        </div>
      </LibraryHeading>
      <AudiobookPagination
        {...navigation}
        limit={book_pagination.limit}
        nextCursor={book_pagination.next_cursor ?? null}
        previousCursor={book_pagination.previous_cursor ?? null}
      />
      {books.length > 0 && (
        <ul className="media-grid" aria-label="Audiobooks">
          {books.map((book) => (
            <li className="media-card" key={book.id}>
              <Artwork
                playlistId={book.id}
                fallback={{
                  id: book.id,
                  title: book.name,
                  artist: "Audiobook",
                  kind: "audiobook",
                  inline: false,
                }}
                playlist
              />
              <Link
                className="media-card-title"
                href={`/audiobooks/${encodeURIComponent(book.id)}`}
              >
                {book.name}
              </Link>
            </li>
          ))}
        </ul>
      )}
      {books.length === 0 && (
        <EmptyState
          title="No audiobooks yet"
          text="Group an editable playlist into ordered chapters and it will appear here."
          libraryLink={false}
        >
          <Link className="button" href="/playlists">
            Browse playlists
          </Link>
        </EmptyState>
      )}
    </>
  );
}
