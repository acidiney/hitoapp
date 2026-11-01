import { useEffect, useRef } from "react";
import { Link } from "@inertiajs/react";
import { Play, Shuffle } from "lucide-react";
import { time } from "../../../shared/format";
import Button from "../../../shared/ui/Button";
import AlbumHeader from "../../catalog/components/AlbumHeader";
import { useCatalogAlbum } from "../hooks/useCatalogAlbum";
import { useCatalogPlayback } from "../hooks/useCatalogPlayback";
import type { ArtistEditionContext } from "../hooks/useArtistEditions";
import { totalMs } from "../model/catalogAlbumRows";
import CatalogCover from "./CatalogCover";
import CatalogTrackTable from "./CatalogTrackTable";

const kinds = {
  album: "Album",
  ep: "EP",
  single: "Single",
  broadcast: "Broadcast",
  other: "Other",
};

export default function CatalogAlbum({
  context,
  heading,
  playOnReady,
}: {
  context: ArtistEditionContext;
  heading: "h4" | "h5";
  playOnReady: boolean;
}) {
  const { group } = context;
  const { editions, tracks, rows, credit } = useCatalogAlbum(context);
  const playback = useCatalogPlayback(
    context,
    editions.chosen?.id ?? null,
    rows.map((row) => row.id),
  );
  const ready = rows.length > 0;
  const started = useRef(false);
  const starter = useRef(playback.start);
  starter.current = playback.start;
  useEffect(() => {
    if (!playOnReady || !ready || started.current) return;
    started.current = true;
    void starter.current(0);
  }, [playOnReady, ready]);
  if (!editions.opened)
    return (
      <Button type="button" onClick={() => void editions.browse()}>
        Open album
      </Button>
    );
  const duration = totalMs(rows);
  const switchable = editions.editions.length > 1 || !!editions.continuation;
  return (
    <section aria-label="Album" aria-busy={editions.pending || tracks.pending}>
      <AlbumHeader
        heading={heading}
        cover={
          <CatalogCover
            group={group.id}
            edition={editions.chosen?.id ?? null}
            title={group.title}
            artist={credit}
          />
        }
        title={group.title}
        credit={credit && <span className="album-artist">{credit}</span>}
        stats={
          <>
            {[
              group.kind ? kinds[group.kind] : null,
              group.first_release_date,
              rows.length > 0
                ? `${rows.length} ${rows.length === 1 ? "track" : "tracks"}${duration > 0 ? ` · ${time(duration / 1000)}` : ""}`
                : null,
            ]
              .filter(Boolean)
              .join(" · ")}
          </>
        }
        actions={
          <>
            <button
              type="button"
              className="button primary"
              aria-label="Play album"
              disabled={!ready}
              onClick={() => void playback.start(0)}
            >
              <Play size={17} fill="currentColor" />
              Play
            </button>
            <button
              type="button"
              className="button"
              aria-label="Shuffle album"
              disabled={!ready}
              onClick={() => void playback.shuffle()}
            >
              <Shuffle size={17} />
              Shuffle
            </button>
          </>
        }
        notes={
          <>
            {playback.consentOff && (
              <p className="album-note" role="status">
                Playing a catalog album searches your connected services, and
                “Search connected services automatically” is off.{" "}
                <Link href="/settings?section=privacy">
                  Turn it on in Settings
                </Link>
              </p>
            )}
            {playback.halted && (
              <p className="album-note" role="alert">
                {playback.halted}
              </p>
            )}
          </>
        }
      />
      {switchable && (
        <div className="edition-switch">
          <label>
            <span>Edition</span>
            <select
              aria-label="Edition"
              value={editions.chosen?.id ?? ""}
              disabled={editions.pending}
              onChange={(event) => editions.choose(event.target.value)}
            >
              {editions.editions.map((edition) => (
                <option key={edition.id} value={edition.id}>
                  {[
                    edition.disambiguation ?? edition.title,
                    edition.release_date,
                    edition.country,
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                </option>
              ))}
            </select>
          </label>
          {!editions.error && editions.continuation && (
            <Button
              type="button"
              disabled={editions.pending}
              onClick={() => void editions.more()}
            >
              Load more editions
            </Button>
          )}
        </div>
      )}
      {(editions.pending || tracks.pending) && (
        <p role="status">Loading tracks…</p>
      )}
      {editions.error && <p role="alert">{editions.error}</p>}
      {editions.error && !editions.restartRequired && (
        <Button
          type="button"
          disabled={editions.pending}
          onClick={() => void editions.retry()}
        >
          Try editions again
        </Button>
      )}
      {(editions.restartRequired || (editions.error && editions.pages > 0)) && (
        <Button
          type="button"
          disabled={editions.pending}
          onClick={() => void editions.restart()}
        >
          Restart editions
        </Button>
      )}
      {tracks.error && (
        <>
          <p role="alert">{tracks.error}</p>
          <Button
            type="button"
            disabled={tracks.pending}
            onClick={() => void tracks.retry()}
          >
            Try track list again
          </Button>
        </>
      )}
      {editions.pages > 0 &&
        editions.editions.length === 0 &&
        !editions.pending &&
        !editions.error && (
          <p>No editions were returned for this release group.</p>
        )}
      {editions.limited && <p>Showing the first 50 pages of editions.</p>}
      {rows.length > 0 && (
        <CatalogTrackTable
          rows={rows}
          states={playback.rows}
          onPlay={(index) => void playback.start(index)}
        />
      )}
    </section>
  );
}
