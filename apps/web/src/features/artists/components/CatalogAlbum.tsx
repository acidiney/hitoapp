import { useEffect, useRef, useState } from "react";
import { Link } from "@inertiajs/react";
import type { Schema } from "../../../shared/schema";
import { useMutation } from "../../../shared/hooks/useMutation";
import AlbumHeader from "../../catalog/components/AlbumHeader";
import AlbumActions from "../../catalog/components/AlbumActions";
import AlbumProgressLine from "../../catalog/components/AlbumProgressLine";
import { albumEyebrow } from "../../catalog/model/albumEyebrow";
import { useCatalogRefreshing } from "../api/catalogCache";
import { useCatalogTracks } from "../hooks/useCatalogTracks";
import type { ArtistEditionContext } from "../hooks/useArtistEditions";
import { statsText } from "../model/catalogAlbumRows";
import CatalogCover from "./CatalogCover";
import CatalogTrackList from "./CatalogTrackList";
import CatalogFailure from "./CatalogFailure";
import EditionToolbar, { type EditionFilter } from "./EditionToolbar";
import { editionLabel } from "../model/editionLabel";
import CatalogPlayNotes from "./CatalogPlayNotes";

export default function CatalogAlbum({
  context,
  group,
  heading,
  playOnReady,
  playlists = [],
  withLibrary = false,
  artistHref,
}: {
  artistHref?: string;
  withLibrary?: boolean;
  context: ArtistEditionContext;
  group: Schema<"CatalogReleaseGroup"> | null;
  heading: "h1" | "h4" | "h5";
  playOnReady: boolean;
  playlists?: Schema<"Playlist">[];
}) {
  const { mutate } = useMutation(context.account);
  const { editions, tracks, rows, credit, playback, linked, controls } =
    useCatalogTracks(context, group, withLibrary, mutate);
  const [chosenFilter, setFilter] = useState<EditionFilter>("all");
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
  const title = group?.title || editions.chosen?.title || "Release";
  const owned = rows.filter((row) => row.recording).length;
  const filter = owned > 0 ? chosenFilter : "all";
  const year = group?.first_release_date?.slice(0, 4);
  const catalogName =
    context.association.provider.kind === "builtin_musicbrainz"
      ? "MusicBrainz"
      : "your catalog service";
  return (
    <section
      className={withLibrary ? "detail-page" : undefined}
      aria-label="Album"
      aria-busy={editions.pending || tracks.pending}
    >
      <AlbumHeader
        eyebrow={albumEyebrow(group?.kind, year)}
        heading={heading}
        cover={
          <CatalogCover
            group={context.releaseGroupId}
            edition={editions.chosen?.id ?? null}
            title={title}
            artist={credit}
          />
        }
        title={title}
        credit={
          credit &&
          (artistHref ? (
            <Link className="album-artist" href={artistHref}>
              {credit}
            </Link>
          ) : (
            <span className="album-artist">{credit}</span>
          ))
        }
        stats={rows.length > 0 && tracks.stats ? statsText(tracks.stats) : null}
        status={
          rows.length === 0 ? null : owned === 0 ? (
            <span className="album-status">Not in your library</span>
          ) : (
            <AlbumProgressLine
              progress={{ owned, total: rows.length, elsewhere: 0 }}
            />
          )
        }
        actions={
          <AlbumActions
            play={{
              disabled: !ready,
              play: () => void playback.start(0),
              shuffle: () => void playback.shuffle(),
            }}
            detailsHref={
              linked ? `/albums/${encodeURIComponent(linked)}/details` : null
            }
            retry={null}
          />
        }
        notes={
          <>
            {refreshing && (
              <p className="album-note" role="status">
                Checking for updates…
              </p>
            )}
            <CatalogPlayNotes
              consentOff={playback.consentOff}
              halted={playback.halted}
            />
          </>
        }
      />
      <div className="album-body">
        {editions.chosen && (
          <EditionToolbar
            name={editionLabel(editions.chosen)}
            total={rows.length}
            owned={owned}
            filter={filter}
            onFilter={setFilter}
            source={catalogName}
            items={[
              ...editions.editions.map((edition) => ({
                id: edition.id,
                label: editionLabel(edition),
                disabled: editions.pending,
                onSelect: () => editions.choose(edition.id),
              })),
              ...(!editions.error && editions.continuation
                ? [
                    {
                      label: "Load more editions",
                      disabled: editions.pending,
                      onSelect: () => void editions.more(),
                    },
                  ]
                : []),
            ]}
          />
        )}
        {(editions.pending || tracks.pending) && (
          <p role="status">Loading tracks…</p>
        )}
        {editions.error && (
          <CatalogFailure
            message={editions.error}
            retryLabel="Try editions again"
            pending={editions.pending}
            onRetry={
              editions.restartRequired ? undefined : () => void editions.retry()
            }
            onRestart={
              editions.restartRequired || editions.pages > 0
                ? () => void editions.restart()
                : undefined
            }
          />
        )}
        {tracks.error && (
          <CatalogFailure
            message={tracks.error}
            retryLabel="Try track list again"
            pending={tracks.pending}
            onRetry={() => void tracks.retry()}
          />
        )}
        {editions.pages > 0 &&
          editions.editions.length === 0 &&
          !editions.pending &&
          !editions.error && (
            <p>No editions were returned for this release group.</p>
          )}
        {editions.limited && <p>Showing the first 50 pages of editions.</p>}
        {rows.length > 0 && (
          <CatalogTrackList
            tracks={rows}
            controls={controls}
            filter={filter}
            playlists={playlists}
            mutate={mutate}
          />
        )}
      </div>
    </section>
  );
}
