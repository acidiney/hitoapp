import { Link } from "@inertiajs/react";
import Artwork from "../features/artwork/components/Artwork";
import EmptyState from "../shared/ui/EmptyState";
import PageHeading from "../shared/ui/PageHeading";
import AudiobookPagination from "../components/audiobooks/AudiobookPagination";
import AudiobookCreate from "../components/audiobooks/AudiobookCreate";
import { useAudiobookPageNavigation } from "../hooks/useAudiobookPageNavigation";
import { useAudiobookGrouping } from "../hooks/useAudiobookGrouping";
import type { AudiobookIndexPageProps } from "../shared/types";

export default function AudiobookIndexPage({
  user,
  books,
  book_pagination,
  grouping_playlists,
}: AudiobookIndexPageProps) {
  const navigation = useAudiobookPageNavigation(user.id, book_pagination);
  const grouping = useAudiobookGrouping(user.id, grouping_playlists, books);
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
      <AudiobookCreate {...grouping} />
      <AudiobookPagination
        {...navigation}
        limit={book_pagination.limit}
        nextCursor={book_pagination.next_cursor ?? null}
        previousCursor={book_pagination.previous_cursor ?? null}
      />
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
