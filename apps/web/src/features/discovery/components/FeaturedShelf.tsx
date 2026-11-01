import type { Schema } from "../../../shared/schema";
import { useEffect, useState } from "react";
import MediaCard from "../../../shared/ui/MediaCard";
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

const COVER_RETRY_MS = 1000;

function FeaturedCover({ entry }: { entry: Schema<"FeaturedEntry"> }) {
  const url = featuredApi.artworkUrl(entry);
  const [attempt, setAttempt] = useState(0);
  const [waiting, setWaiting] = useState(false);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    if (!waiting) return;
    const timer = setTimeout(() => {
      setAttempt((value) => value + 1);
      setWaiting(false);
    }, COVER_RETRY_MS);
    return () => clearTimeout(timer);
  }, [waiting]);
  function missed() {
    if (attempt === 0) setWaiting(true);
    else setFailed(true);
  }
  return (
    <>
      <FallbackCover id={entry.id} title={entry.title} inline={false} />
      {url && !failed && !waiting && (
        <img key={attempt} src={url} alt="" loading="lazy" onError={missed} />
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
      <ul className="shelf-row">
        {items.map((entry) => {
          const key = featuredKey(entry);
          const busy = playback.pending === key;
          return (
            <li key={key} aria-busy={busy || undefined}>
              <MediaCard
                title={entry.title}
                subtitle={
                  busy
                    ? "Adding a copy…"
                    : [meta(entry), entry.subtitle].filter(Boolean).join(" · ")
                }
                cover={<FeaturedCover entry={entry} />}
                openLabel={`Open ${entry.title}`}
                onOpen={() => onOpen(entry)}
                onPlay={() => onPlay(entry)}
                disabled={playback.pending !== null}
              />
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
