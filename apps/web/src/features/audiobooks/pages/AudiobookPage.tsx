import type { Schema } from "../../../shared/schema";
import { useRef } from "react";
import Artwork from "../../artwork/components/Artwork";
import Breadcrumbs from "../../../shared/ui/Breadcrumbs";
import CollectionHeader from "../../../shared/ui/CollectionHeader";
import EmptyState from "../../../shared/ui/EmptyState";
import ProgressBar from "../../../shared/ui/ProgressBar";
import { tintFor } from "../../../shared/ui/collectionTint";
import { Link } from "@inertiajs/react";
import AudiobookBookmarkForm from "../components/AudiobookBookmarkForm";
import AudiobookBookmarkList from "../components/AudiobookBookmarkList";
import AudiobookChapters from "../components/AudiobookChapters";
import { longDuration, time } from "../../../shared/format";
import { useAudiobookBookmarks } from "../hooks/useAudiobookBookmarks";
import { useAudiobookPlayback } from "../hooks/useAudiobookPlayback";
import { audiobookProgress } from "../model/audiobookProgress";

import Button from "../../../shared/ui/Button";

export default function AudiobookPage({
  user,
  detail: rendered,
}: Schema<"AudiobookPageProps">) {
  const bookmarks = useAudiobookBookmarks(user.id, rendered);
  const detail = bookmarks.detail;
  const playback = useAudiobookPlayback(user.id, detail, rendered);
  const side = useRef<HTMLElement>(null);
  const count = detail.chapters.length;
  const progress = audiobookProgress(detail);
  const authors = new Set(detail.chapters.map((chapter) => chapter.artist));
  const author = authors.size === 1 ? [...authors][0] : "";
  const total = detail.chapters.reduce(
    (sum, chapter) => sum + Math.max(0, chapter.duration_ms),
    0,
  );
  const continuing = detail.continuation?.progress;
  const continuingNumber =
    detail.chapters.findIndex(
      (chapter) => chapter.entry_id === detail.continuation?.chapter.entry_id,
    ) + 1;
  const durationsKnown = detail.chapters.every(
    (chapter) => chapter.duration_ms > 0,
  );
  const bookmarkLock = bookmarks.pending || bookmarks.hasAttempt;
  const bookmarkable = detail.chapters.some(
    (chapter) => chapter.recording_id != null && chapter.available,
  );
  const bookmarkReason = !bookmarkable
    ? "Bookmarks need a chapter that is available to play."
    : bookmarkLock
      ? "A bookmark change is in progress or needs a retry."
      : undefined;
  return (
    <>
      <CollectionHeader
        tint={tintFor(detail.playlist.id)}
        size="audiobook"
        crumbs={
          <Breadcrumbs
            crumbs={[
              { label: "Library", href: "/library" },
              { label: "Audiobooks", href: "/audiobooks" },
              { label: detail.playlist.name },
            ]}
          />
        }
        cover={
          <Artwork
            playlistId={detail.playlist.id}
            recording={detail.chapters[0]?.recording_id ?? undefined}
            fallback={{
              id: detail.playlist.id,
              title: detail.playlist.name,
              artist: author || "Audiobook",
            }}
            playlist
          />
        }
        eyebrow={`Audiobook · ${count} ${count === 1 ? "chapter" : "chapters"}${
          total > 0
            ? ` · ${durationsKnown ? "" : "≥ "}${longDuration(total / 1000)}`
            : ""
        }`}
        title={detail.playlist.name}
        stats={author}
        extra={
          progress?.overall && (
            <div className="audiobook-progress">
              <ProgressBar
                percent={progress.overall.percent}
                label="Position in the audiobook"
              />
              <span className="media-mono">
                {Math.round(progress.overall.percent)}% ·{" "}
                {longDuration(progress.overall.leftMs / 1000)} left
              </span>
            </div>
          )
        }
        actions={
          <>
            {continuing && (
              <Button
                className="tint-primary"
                type="button"
                disabled={playback.pending}
                onClick={() => void playback.continueAudiobook()}
              >
                {continuingNumber > 0
                  ? `Continue ch. ${continuingNumber} · `
                  : "Continue · "}
                {time(continuing.position_ms / 1000)}
              </Button>
            )}
            <Button
              className="tint-secondary"
              type="button"
              disabled={bookmarkReason !== undefined}
              title={bookmarkReason}
              onClick={() =>
                side.current?.querySelector<HTMLElement>("select")?.focus()
              }
            >
              Bookmark
            </Button>
            <Link
              className="button tint-secondary"
              href={`/playlists/${encodeURIComponent(detail.playlist.id)}`}
            >
              Open playlist
            </Link>
          </>
        }
      />
      <div className="media-layout">
        <section className="media-main" aria-labelledby="chapters-heading">
          <div className="media-section-head">
            <h2 id="chapters-heading">Chapters</h2>
          </div>
          {count > 0 ? (
            <AudiobookChapters
              chapters={detail.chapters}
              currentEntry={detail.continuation?.chapter.entry_id ?? null}
              currentPositionMs={continuing?.position_ms ?? null}
              disabled={playback.pending}
              play={playback.playChapter}
            />
          ) : (
            <EmptyState
              title="No chapters yet"
              text="This audiobook follows the chapters in its playlist."
              libraryLink={false}
            />
          )}
          {playback.error && <p role="alert">{playback.error}</p>}
        </section>
        <section className="media-side" ref={side} aria-label="Bookmarks">
          <h2>Bookmarks</h2>
          <AudiobookBookmarkList
            detail={detail}
            disabled={bookmarks.pending || bookmarks.hasAttempt}
            remove={bookmarks.remove}
            play={playback.playBookmark}
            playbackPending={playback.pending}
          />
          <div className="media-card-panel">
            <h3>Add a bookmark</h3>
            <AudiobookBookmarkForm chapters={detail.chapters} {...bookmarks} />
          </div>
        </section>
      </div>
    </>
  );
}
