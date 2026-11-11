import type { Schema } from "../../../shared/schema";
import { Play, Plus } from "lucide-react";
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

function meta(entry: Schema<"FeaturedEntry">, service: string): string {
  return [entry.subtitle || KIND_LABEL[entry.kind], service]
    .filter(Boolean)
    .join(" · ");
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
  services,
  playback,
  onPlay,
  onOpen,
}: {
  items: Schema<"FeaturedEntry">[];
  services: Record<string, string>;
  playback: FeaturedPlaybackState;
  onPlay: (entry: Schema<"FeaturedEntry">) => void;
  onOpen: (entry: Schema<"FeaturedEntry">) => void;
}) {
  if (items.length === 0) return null;
  const names = [
    ...new Set(items.map((entry) => services[entry.installation_id])),
  ].filter(Boolean);
  const anywhere = names.length === 0 ? "your services" : names.join(" and ");
  return (
    <section className="featured-block" aria-labelledby="discover-featured">
      <div className="featured-head">
        <h3 id="discover-featured">Featured on {anywhere}</h3>
        <span>Chosen by the plugin, not by Hito.</span>
        <span>Playing or opening a card saves it to your Playlists.</span>
      </div>
      <ul className="featured-grid">
        {items.map((entry) => {
          const key = featuredKey(entry);
          const busy = playback.pending === key;
          const service = services[entry.installation_id] ?? "";
          return (
            <li
              key={key}
              className="featured-card"
              aria-busy={busy || undefined}
            >
              <span className="featured-cover" aria-hidden="true">
                <FeaturedCover entry={entry} />
              </span>
              <span className="featured-copy">
                <span className="featured-title">{entry.title}</span>
                <span className="featured-sub">
                  {busy ? "Adding a copy…" : meta(entry, service)}
                </span>
              </span>
              <button
                type="button"
                className="featured-action"
                aria-label={`Play ${entry.title}`}
                disabled={playback.pending !== null}
                onClick={() => onPlay(entry)}
              >
                <Play size={14} fill="currentColor" aria-hidden="true" />
              </button>
              <button
                type="button"
                className="featured-action"
                aria-label={`Add ${entry.title} to your playlists`}
                disabled={playback.pending !== null}
                onClick={() => onOpen(entry)}
              >
                <Plus size={16} aria-hidden="true" />
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
