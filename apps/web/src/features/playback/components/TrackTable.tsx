import type { Schema } from "../../../shared/schema";
import { useSyncExternalStore } from "react";
import { Link } from "@inertiajs/react";
import { time } from "../../../shared/format";
import { usePlayer } from "./PlaybackProvider";
import type { PlaybackStart, Track } from "../model";

import type { Mutate } from "../../../shared/hooks/useMutation";
import TrackRowActions from "./TrackRowActions";
import PlaylistEntryRemove from "./PlaylistEntryRemove";
import EntryMoves from "./EntryMoves";
import RowPlay from "./RowPlay";
import TrackAvailabilityNote from "./TrackAvailabilityNote";
import CatalogRowActions, { CatalogRowNote } from "./CatalogRowActions";
import TrackNames, { artistHref, albumHref } from "./TrackNames";
import { trackHref } from "./TrackRowMenu";
import Artwork from "../../artwork/components/Artwork";
import type { TrackListing } from "../api/trackListing";
import { usePlayTracks } from "../hooks/usePlayTracks";

export type TrackRow = Track & {
  row_id: string;
  liked: boolean;
  album?: string | null;
  release_id?: string | null;
  artist_id?: string | null;
  disc_number?: number | null;
  track_number?: number | null;
  playback_start?: PlaybackStart;
  entry_id?: string;
};

export type CatalogRowView = {
  id: string;
  number: string;
  title: string;
  artist: string;
  album: string | null;
  durationMs: number | null;
  status: "idle" | "finding" | "found" | "not_found" | "failed";
  recording: TrackRow | null;
};

export type CatalogControls = {
  play(index: number): void;
  find(index: number): void;
  open(index: number): void;
  like(index: number): void;
  mutated(action: Schema<"Action">): void;
};

export type TrackTableRow =
  | { kind: "recording"; track: TrackRow }
  | { kind: "catalog"; row: CatalogRowView }
  | {
      kind: "retained";
      entry: Pick<
        Schema<"PlaylistEntry">,
        "entry_id" | "title" | "artist" | "origin"
      >;
    };

export type PlaylistEdit = {
  active: boolean;
  busy: boolean;
  removed: ReadonlySet<string>;
  move(entryId: string, delta: -1 | 1): void;
  toggleRemove(entryId: string): void;
};

export type TrackColumn = "artist" | "album";

function trackNumber(track: TrackRow, fallback: number): string | number {
  if (track.track_number == null) return fallback;
  const disc =
    track.disc_number && track.disc_number > 1 ? `${track.disc_number}.` : "";
  return `${disc}${track.track_number}`;
}

function retainedNote(origin: Schema<"PlaylistEntry">["origin"]): string {
  if (origin?.kind === "unknown") return "Unknown item from its provider";
  if (origin?.kind === "unavailable") return "Unavailable from its provider";
  return "Unavailable recording";
}

export default function TrackTable({
  rows,
  offset = 0,
  playlists,
  playlist,
  mutate,
  listing,
  edit,
  catalog,
  without = [],
}: {
  catalog?: CatalogControls;
  edit?: PlaylistEdit;
  rows: TrackTableRow[];
  listing?: TrackListing;
  offset?: number;
  playlists: Schema<"Playlist">[];
  playlist?: Schema<"Playlist">;
  mutate: Mutate;
  without?: TrackColumn[];
}) {
  const player = usePlayer();
  const currentId = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().current?.id,
  );
  const tracks = rows.flatMap((row) =>
    row.kind === "recording" ? [row.track] : [],
  );
  const playTracks = usePlayTracks(tracks, listing);
  const queuePositions = new Map<string, number>(
    tracks.map((track, index) => [track.row_id, index]),
  );
  function play(track: TrackRow) {
    const index = queuePositions.get(track.row_id);
    if (index !== undefined)
      void playTracks.playFrom(index, track.playback_start);
  }
  const anyAlbum = rows.some((row) =>
    row.kind === "recording"
      ? row.track.album != null
      : row.kind === "catalog"
        ? row.row.album != null || row.row.recording?.album != null
        : false,
  );
  const showArtist = !without.includes("artist");
  const showAlbum = !without.includes("album") && anyAlbum;
  return (
    <div className="table-scroll">
      <table
        className="track-table"
        aria-label={playlist ? "Playlist entries" : "Tracks"}
      >
        <thead>
          <tr>
            <th className="number-column">
              <span className="sr-only">Play</span>#
            </th>
            <th>Title</th>
            {showArtist && <th className="artist-column">Artist</th>}
            {showAlbum && <th className="album-column">Album</th>}
            <th className="duration-column">Time</th>
            <th>
              <span className="sr-only">Actions</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row, index) => {
            const track =
              row.kind === "recording"
                ? row.track
                : row.kind === "catalog"
                  ? row.row.recording
                  : null;
            const view = row.kind === "catalog" ? row.row : null;
            const display =
              row.kind === "recording"
                ? row.track
                : row.kind === "catalog"
                  ? row.row
                  : row.entry;
            const rowId =
              row.kind === "recording"
                ? row.track.row_id
                : row.kind === "catalog"
                  ? row.row.id
                  : row.entry.entry_id;
            const album = track?.album ?? view?.album ?? null;
            const albumLink = track ? albumHref(track) : null;
            const artistLink = track ? artistHref(track) : null;
            const duration =
              track !== null
                ? track.duration_ms
                : view !== null
                  ? view.durationMs
                  : null;
            return (
              <tr
                key={rowId}
                className={[
                  track !== null && currentId === track.id ? "playing-row" : "",
                  edit?.removed.has(rowId) ? "removed-row" : "",
                ]
                  .filter(Boolean)
                  .join(" ")}
              >
                <td className="number-column">
                  {view && catalog ? (
                    <RowPlay
                      title={view.title}
                      number={view.number}
                      onPlay={() => catalog.play(index)}
                    />
                  ) : track ? (
                    <RowPlay
                      title={track.title}
                      number={trackNumber(track, index + 1 + offset)}
                      onPlay={() => play(track)}
                    />
                  ) : (
                    <span className="row-number">{index + 1 + offset}</span>
                  )}
                </td>
                <td className="track-cell">
                  {view && !track ? (
                    <Artwork
                      fallback={{
                        id: view.id,
                        title: view.album ?? view.title,
                        artist: view.artist,
                      }}
                    />
                  ) : track ? (
                    <Artwork
                      release={track.release_id ?? undefined}
                      recording={track.id}
                      fallback={{
                        id: track.release_id ?? track.id,
                        title: track.album ?? track.title,
                        artist: track.artist,
                      }}
                    />
                  ) : (
                    <Artwork />
                  )}
                  <div className="track-copy">
                    {track ? (
                      <Link className="track-title" href={trackHref(track.id)}>
                        {track.title}
                      </Link>
                    ) : view ? (
                      <span className="track-title">{view.title}</span>
                    ) : (
                      <span className="retained-title">{display.title}</span>
                    )}
                    {view && !track ? (
                      <CatalogRowNote status={view.status} />
                    ) : row.kind === "retained" ? (
                      <small>{retainedNote(row.entry.origin)}</small>
                    ) : (
                      <TrackAvailabilityNote track={track!} />
                    )}
                    <TrackNames
                      className="track-meta"
                      artist={showArtist ? display.artist : undefined}
                      artistHref={artistLink}
                      album={showAlbum ? album : null}
                      albumHref={albumLink}
                    />
                  </div>
                </td>
                {showArtist && (
                  <td className="artist-column muted">
                    <TrackNames
                      artist={display.artist}
                      artistHref={artistLink}
                    />
                  </td>
                )}
                {showAlbum && (
                  <td className="album-column muted">
                    <TrackNames album={album} albumHref={albumLink} />
                  </td>
                )}
                <td className="duration-column muted">
                  {duration == null ? "—" : time(duration / 1000)}
                </td>
                <td className="actions-column">
                  {view && !track && catalog ? (
                    <CatalogRowActions
                      title={view.title}
                      status={view.status}
                      onFind={() => catalog.find(index)}
                      onOpen={() => catalog.open(index)}
                      onLike={() => catalog.like(index)}
                    />
                  ) : track ? (
                    <TrackRowActions
                      track={track}
                      playlists={playlists}
                      playlist={playlist}
                      mutate={mutate}
                      onMutationAccepted={catalog?.mutated}
                      replaceMenu={
                        edit?.active && playlist?.kind === "local" ? (
                          <PlaylistEntryRemove
                            title={track.title}
                            removed={edit.removed.has(rowId)}
                            disabled={edit.busy}
                            onToggle={() => edit.toggleRemove(rowId)}
                          />
                        ) : undefined
                      }
                    >
                      {edit?.active && (
                        <EntryMoves
                          entryId={track.entry_id ?? track.row_id}
                          title={track.title}
                          first={index === 0}
                          last={index === rows.length - 1}
                          removed={edit.removed.has(rowId)}
                          edit={edit}
                        />
                      )}
                    </TrackRowActions>
                  ) : row.kind === "retained" && playlist && edit?.active ? (
                    <div className="track-actions">
                      <EntryMoves
                        entryId={row.entry.entry_id}
                        title={row.entry.title}
                        first={index === 0}
                        last={index === rows.length - 1}
                        removed={edit.removed.has(rowId)}
                        edit={edit}
                      />
                      {playlist.kind === "local" && (
                        <PlaylistEntryRemove
                          title={row.entry.title}
                          removed={edit.removed.has(rowId)}
                          disabled={edit.busy}
                          onToggle={() => edit.toggleRemove(rowId)}
                        />
                      )}
                    </div>
                  ) : null}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
