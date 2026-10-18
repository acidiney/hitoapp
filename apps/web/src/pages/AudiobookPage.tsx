import { Link } from "@inertiajs/react";
import EmptyState from "../components/EmptyState";
import PageHeading from "../components/PageHeading";
import { time } from "../format";
import type { AudiobookPageProps } from "../types";

export default function AudiobookPage({ detail }: AudiobookPageProps) {
  const chapters = new Map(
    detail.chapters.map(
      (entry, index) => [entry.entry_id, { entry, number: index + 1 }] as const,
    ),
  );
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
        {detail.bookmarks.length > 0 ? (
          <ol>
            {detail.bookmarks.map((bookmark) => {
              const chapter = chapters.get(bookmark.entry_id);
              const anchored =
                chapter?.entry.recording_id === bookmark.recording_id
                  ? chapter
                  : undefined;
              return (
                <li key={bookmark.id}>
                  <strong>{bookmark.label}</strong>
                  <p>
                    {anchored
                      ? `Chapter ${anchored.number}: ${anchored.entry.title || "Untitled chapter"}`
                      : "Chapter unavailable"}
                  </p>
                  <span>{time(bookmark.position_ms / 1000)}</span>
                  {anchored && !anchored.entry.available && (
                    <p>Chapter unavailable</p>
                  )}
                </li>
              );
            })}
          </ol>
        ) : (
          <p>No bookmarks yet.</p>
        )}
      </section>
    </>
  );
}
