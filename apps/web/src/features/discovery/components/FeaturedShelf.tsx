import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import { Play } from "lucide-react";
import FallbackCover from "../../../shared/ui/FallbackCover";
import { featuredApi } from "../api/featuredApi";
import {
  featuredKey,
  type FeaturedPlaybackState,
} from "../hooks/useFeaturedPlayback";

const KIND_LABEL: Record<Schema<"FeaturedKind">, string> = {
  playlist: "Playlist",
  album: "Album",
  new_release: "New release",
};

function meta(entry: Schema<"FeaturedEntry">): string {
  const kind = KIND_LABEL[entry.kind];
  if (entry.item_count == null) return kind;
  return `${kind} · ${entry.item_count} ${entry.item_count === 1 ? "track" : "tracks"}`;
}

function FeaturedCover({ entry }: { entry: Schema<"FeaturedEntry"> }) {
  const url = featuredApi.artworkUrl(entry);
  const [failed, setFailed] = useState(false);
  return (
    <>
      <FallbackCover id={entry.id} title={entry.title} inline={false} />
      {url && !failed && (
        <img src={url} alt="" loading="lazy" onError={() => setFailed(true)} />
      )}
    </>
  );
}

export default function FeaturedShelf({
  items,
  playback,
  onPlay,
  onOpen,
}: {
  items: Schema<"FeaturedEntry">[];
  playback: FeaturedPlaybackState;
  onPlay: (entry: Schema<"FeaturedEntry">) => void;
  onOpen: (entry: Schema<"FeaturedEntry">) => void;
}) {
  if (items.length === 0) return null;
  return (
    <section className="discover-section" aria-labelledby="discover-featured">
      <h2 id="discover-featured">From your services</h2>
      <ul className="featured-row">
        {items.map((entry) => {
          const key = featuredKey(entry);
          const busy = playback.pending === key;
          return (
            <li
              key={key}
              className="featured-card"
              aria-busy={busy || undefined}
            >
              <div className="featured-card-cover">
                <FeaturedCover entry={entry} />
                <button
                  type="button"
                  className="featured-card-play"
                  aria-label={`Play ${entry.title}`}
                  disabled={playback.pending !== null}
                  onClick={() => onPlay(entry)}
                >
                  <Play size={18} fill="currentColor" />
                </button>
              </div>
              <button
                type="button"
                className="featured-card-open"
                aria-label={`Open ${entry.title}`}
                disabled={playback.pending !== null}
                onClick={() => onOpen(entry)}
              >
                <span className="featured-card-title">{entry.title}</span>
                {entry.subtitle && (
                  <span className="featured-card-subtitle">
                    {entry.subtitle}
                  </span>
                )}
                <span className="featured-card-meta">
                  {busy ? "Adding a copy…" : meta(entry)}
                </span>
              </button>
            </li>
          );
        })}
      </ul>
      {playback.notice &&
        (playback.notice.tone === "error" ? (
          <p className="radio-notice" role="alert" data-tone="error">
            {playback.notice.text}
          </p>
        ) : (
          <p className="radio-notice" role="status">
            {playback.notice.text}
          </p>
        ))}
    </section>
  );
}
