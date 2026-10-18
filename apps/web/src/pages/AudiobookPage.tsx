import { Link } from "@inertiajs/react";
import EmptyState from "../components/EmptyState";
import PageHeading from "../components/PageHeading";
import AudiobookBookmarkForm from "../components/audiobooks/AudiobookBookmarkForm";
import AudiobookBookmarkList from "../components/audiobooks/AudiobookBookmarkList";
import { time } from "../format";
import { useAudiobookBookmarks } from "../hooks/useAudiobookBookmarks";
import type { AudiobookPageProps } from "../types";

export default function AudiobookPage({
  user,
  detail: rendered,
}: AudiobookPageProps) {
  const bookmarks = useAudiobookBookmarks(user.id, rendered);
  const detail = bookmarks.detail;
  const count = detail.chapters.length;
  return (
    <>
      <PageHeading
        title={detail.playlist.name}
        description={`${count} ${count === 1 ? "chapter" : "chapters"}`}
      >
        <Link className="button" href="/audiobooks">
          All audiobooks
        </Link>
        <Link
          className="button"
          href={`/playlists/${encodeURIComponent(detail.playlist.id)}`}
        >
          Open playlist
        </Link>
      </PageHeading>
      {count > 0 ? (
        <div className="table-scroll">
          <table className="track-table" aria-label="Chapters">
            <thead>
              <tr>
                <th scope="col">Chapter</th>
                <th scope="col">Title</th>
                <th scope="col">Narrator</th>
                <th scope="col">Duration</th>
                <th scope="col">Availability</th>
              </tr>
            </thead>
            <tbody>
              {detail.chapters.map((chapter, index) => (
                <tr key={chapter.entry_id} id={`chapter-${chapter.entry_id}`}>
                  <th scope="row">Chapter {index + 1}</th>
                  <td>{chapter.title || "Untitled chapter"}</td>
                  <td>{chapter.artist || "—"}</td>
                  <td>
                    {chapter.duration_ms > 0
                      ? time(chapter.duration_ms / 1000)
                      : "—"}
                  </td>
                  <td>
                    {chapter.recording_id != null && chapter.available
                      ? "Available"
                      : "Chapter unavailable"}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : (
        <EmptyState
          title="No chapters yet"
          text="This audiobook follows the chapters in its playlist."
          libraryLink={false}
        />
      )}
      <section aria-label="Bookmarks">
        <h2>Bookmarks</h2>
        <AudiobookBookmarkForm chapters={detail.chapters} {...bookmarks} />
        <AudiobookBookmarkList
          detail={detail}
          disabled={bookmarks.pending || bookmarks.hasAttempt}
          remove={bookmarks.remove}
        />
      </section>
    </>
  );
}
