import { useEffect, useRef, type ReactNode } from "react";
import { Link } from "@inertiajs/react";
import { Play, Shuffle } from "lucide-react";
import type { Schema } from "../../../shared/schema";
import { time } from "../../../shared/format";
import { useMutation } from "../../../shared/hooks/useMutation";
import Button from "../../../shared/ui/Button";
import AlbumHeader from "../../catalog/components/AlbumHeader";
import TrackTable from "../../playback/components/TrackTable";
import { useCatalogRefreshing } from "../api/catalogCache";
import { useCatalogTracks } from "../hooks/useCatalogTracks";
import type { ArtistEditionContext } from "../hooks/useArtistEditions";
import { totalMs } from "../model/catalogAlbumRows";
import CatalogCover from "./CatalogCover";
import CatalogEditionSwitch from "./CatalogEditionSwitch";
import LibraryAvailability from "./LibraryAvailability";

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
  playlists = [],
  crumbs,
  withLibrary = false,
}: {
  withLibrary?: boolean;
  context: ArtistEditionContext;
  heading: "h1" | "h4" | "h5";
  playOnReady: boolean;
  playlists?: Schema<"Playlist">[];
  crumbs?: ReactNode;
}) {
  const { group } = context;
  const { mutate } = useMutation(context.account);
  const {
    editions,
    tracks,
    rows,
    occurrences,
    credit,
    playback,
    library,
    controls,
  } = useCatalogTracks(context, withLibrary, mutate);
  const owned = library.albums[group.id] ?? [];
  const refreshing = useCatalogRefreshing();
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
  const title = group.title || editions.chosen?.title || "Release";
  const duration = totalMs(occurrences);
  const switchable = editions.editions.length > 1 || !!editions.continuation;
  return (
    <section aria-label="Album" aria-busy={editions.pending || tracks.pending}>
      <AlbumHeader
        crumbs={crumbs}
        eyebrow={group.kind ? kinds[group.kind] : "Release"}
        heading={heading}
        cover={
          <CatalogCover
            group={group.id}
            edition={editions.chosen?.id ?? null}
            title={title}
            artist={credit}
          />
        }
        title={title}
        credit={credit && <span className="album-artist">{credit}</span>}
        stats={
          <>
            {[
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
            {switchable && (
              <CatalogEditionSwitch
                editions={editions.editions}
                chosen={editions.chosen?.id ?? ""}
                pending={editions.pending}
                more={!editions.error && !!editions.continuation}
                onChoose={editions.choose}
                onMore={() => void editions.more()}
              />
            )}
            {withLibrary && owned.length > 0 && (
              <LibraryAvailability
                account={context.account}
                albums={owned}
                group={group.id}
              />
            )}
            {refreshing && (
              <p className="album-note quiet" role="status">
                Checking for updates…
              </p>
            )}
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
        <TrackTable
          rows={rows}
          catalog={controls}
          playlists={playlists}
          mutate={mutate}
        />
      )}
    </section>
  );
}
