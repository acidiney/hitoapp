import type { Schema } from "../../../shared/schema";
import MediaCard from "../../../shared/ui/MediaCard";
import FallbackCover from "../../../shared/ui/FallbackCover";
import { useArtworkRetry } from "../../../shared/hooks/useArtworkRetry";
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

const COVER_RETRY_MS = [1000];

function FeaturedCover({ entry }: { entry: Schema<"FeaturedEntry"> }) {
  const url = featuredApi.artworkUrl(entry);
  const retry = useArtworkRetry(COVER_RETRY_MS, url ?? "");
  return (
    <>
      <FallbackCover
        id={entry.id}
        title={entry.title}
        kind={entry.kind === "playlist" ? "playlist" : "album"}
        inline={false}
      />
      {url && !retry.exhausted && !retry.waiting && (
        <img
          key={retry.attempt}
          src={url}
          alt=""
          loading="lazy"
          onError={retry.miss}
        />
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
      <p className="discover-caption">
        Playing or opening a card saves it to your Playlists.
      </p>
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
