import type { Schema } from "../../../shared/schema";
import { useSyncExternalStore } from "react";
import { Link } from "@inertiajs/react";
import { time } from "../../../shared/format";
import { usePlayer } from "./PlaybackProvider";
import type { PlaybackStart, Track } from "../model";

import type { Mutate } from "../../../shared/hooks/useMutation";
import TrackActions, { PlaylistEntryRemove } from "./TrackActions";
import EntryMoves from "./EntryMoves";
import RowPlay from "./RowPlay";
import TrackAvailabilityNote from "./TrackAvailabilityNote";
import CatalogRowActions, { CatalogRowNote } from "./CatalogRowActions";
import Artwork from "../../artwork/components/Artwork";
import type { TrackListing } from "../api/trackListing";
import { usePlayTracks } from "../hooks/usePlayTracks";

export type TrackRow = Track & {
  row_id: string;
  liked: boolean;
  album?: string | null;
  release_id?: string | null;
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
  move(entryId: string, delta: -1 | 1): void;
};

export default function TrackTable({
  rows,
  offset = 0,
  playlists,
  playlist,
  mutate,
  listing,
  edit,
  catalog,
}: {
  catalog?: CatalogControls;
  edit?: PlaylistEdit;
  rows: TrackTableRow[];
  listing?: TrackListing;
  offset?: number;
  playlists: Schema<"Playlist">[];
  playlist?: Schema<"Playlist">;
  mutate: Mutate;
}) {
  const player = usePlayer();
  const currentId = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().current?.id,
  );
  const tracks = rows.flatMap((row) =>
    row.kind === "recording" ? [row.track] : [],
  );
  const resolved = (row: TrackTableRow) =>
    row.kind === "catalog" ? row.row.recording : null;
  const playTracks = usePlayTracks(tracks, listing);
  const queuePositions = new Map<string, number>(
    tracks.map((track, index) => [track.row_id, index]),
  );
  function play(track: TrackRow) {
    const index = queuePositions.get(track.row_id);
    if (index !== undefined)
      void playTracks.playFrom(index, track.playback_start);
  }
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
            <th className="artist-column">Artist</th>
            <th className="album-column">Album</th>
            <th className="duration-column">Time</th>
            <th>
              <span className="sr-only">Actions</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row, index) => {
            const track = row.kind === "recording" ? row.track : resolved(row);
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
            return (
              <tr
                key={rowId}
                className={
                  track !== null && currentId === track.id ? "playing-row" : ""
                }
              >
                <td>
                  {view && catalog ? (
                    <RowPlay
                      title={view.title}
                      number={view.number}
                      onPlay={() => catalog.play(index)}
                    />
                  ) : track ? (
                    <RowPlay
                      title={track.title}
                      number={
                        track.track_number != null
                          ? `${track.disc_number && track.disc_number > 1 ? `${track.disc_number}.` : ""}${track.track_number}`
                          : index + 1 + offset
                      }
                      onPlay={() => play(track)}
                    />
                  ) : (
                    <span>{index + 1 + offset}</span>
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
                  <div>
                    {view && catalog ? (
                      <button
                        className="track-title"
                        onClick={() => catalog.play(index)}
                      >
                        {view.title}
                      </button>
                    ) : track ? (
                      <button
                        className="track-title"
                        onClick={() => play(track)}
                      >
                        {track.title}
                      </button>
                    ) : (
                      <span className="retained-title">{display.title}</span>
                    )}
                    {view && !track ? (
                      <CatalogRowNote status={view.status} />
                    ) : row.kind === "retained" ? (
                      <small>
                        {row.entry.origin?.kind === "unknown"
                          ? "Unknown item from its provider"
                          : row.entry.origin?.kind === "unavailable"
                            ? "Unavailable from its provider"
                            : "Unavailable recording"}
                      </small>
                    ) : (
                      <TrackAvailabilityNote track={track!} />
                    )}
                    <span className="mobile-artist">{display.artist}</span>
                  </div>
                </td>
                <td className="artist-column muted">{display.artist}</td>
                <td className="album-column muted">
                  {track?.release_id ? (
                    <Link
                      href={`/albums/${encodeURIComponent(track.release_id)}`}
                    >
                      {track.album ?? "Unknown album"}
                    </Link>
                  ) : (
                    (track?.album ?? view?.album ?? "—")
                  )}
                </td>
                <td className="duration-column muted">
                  {view && !track
                    ? view.durationMs == null
                      ? "—"
                      : time(view.durationMs / 1000)
                    : track
                      ? time(track.duration_ms / 1000)
                      : "—"}
                </td>
                <td>
                  {view && !track && catalog ? (
                    <CatalogRowActions
                      title={view.title}
                      status={view.status}
                      onFind={() => catalog.find(index)}
                      onOpen={() => catalog.open(index)}
                      onLike={() => catalog.like(index)}
                    />
                  ) : track ? (
                    <TrackActions
                      track={track}
                      playlists={playlists}
                      playlist={playlist}
                      editing={edit?.active ?? false}
                      mutate={mutate}
                      onMutationAccepted={catalog?.mutated}
                    >
                      {edit?.active && (
                        <EntryMoves
                          entryId={track.entry_id ?? track.row_id}
                          title={track.title}
                          first={index === 0}
                          last={index === rows.length - 1}
                          edit={edit}
                        />
                      )}
                    </TrackActions>
                  ) : row.kind === "retained" && playlist && edit?.active ? (
                    <div className="track-actions">
                      <EntryMoves
                        entryId={row.entry.entry_id}
                        title={row.entry.title}
                        first={index === 0}
                        last={index === rows.length - 1}
                        edit={edit}
                      />
                      <PlaylistEntryRemove
                        entryId={row.entry.entry_id}
                        title={row.entry.title}
                        playlist={playlist}
                        mutate={mutate}
                      />
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
