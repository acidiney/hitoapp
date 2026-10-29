import { useSyncExternalStore } from "react";
import { Link } from "@inertiajs/react";
import { Play } from "lucide-react";
import { time } from "../../../shared/format";
import { usePlayer } from "./PlaybackProvider";
import type { PlaybackStart, Track } from "../model";
import type { Playlist, PlaylistEntry } from "../../../shared/types";
import type { Mutate } from "../../../shared/hooks/useMutation";
import TrackActions, { PlaylistEntryRemove } from "./TrackActions";
import EntryMoves from "./EntryMoves";
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

export type TrackTableRow =
  | { kind: "recording"; track: TrackRow }
  | {
      kind: "retained";
      entry: Pick<PlaylistEntry, "entry_id" | "title" | "artist" | "origin">;
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
}: {
  edit?: PlaylistEdit;
  rows: TrackTableRow[];
  listing?: TrackListing;
  offset?: number;
  playlists: Playlist[];
  playlist?: Playlist;
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
            const track = row.kind === "recording" ? row.track : null;
            const display = row.kind === "recording" ? row.track : row.entry;
            const rowId =
              row.kind === "recording" ? row.track.row_id : row.entry.entry_id;
            return (
              <tr
                key={rowId}
                className={
                  track !== null && currentId === track.id ? "playing-row" : ""
                }
              >
                <td>
                  {track ? (
                    <button
                      className="row-play icon-button"
                      aria-label={`Play ${track.title}`}
                      onClick={() => play(track)}
                    >
                      <span>
                        {track.track_number != null
                          ? `${track.disc_number && track.disc_number > 1 ? `${track.disc_number}.` : ""}${track.track_number}`
                          : index + 1 + offset}
                      </span>
                      <Play size={15} />
                    </button>
                  ) : (
                    <span>{index + 1 + offset}</span>
                  )}
                </td>
                <td className="track-cell">
                  {track ? (
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
                    {track ? (
                      <button
                        className="track-title"
                        onClick={() => play(track)}
                      >
                        {track.title}
                      </button>
                    ) : (
                      <span className="retained-title">{display.title}</span>
                    )}
                    {row.kind === "retained" ? (
                      <small>
                        {row.entry.origin?.kind === "unknown"
                          ? "Unknown item from its provider"
                          : row.entry.origin?.kind === "unavailable"
                            ? "Unavailable from its provider"
                            : "Unavailable recording"}
                      </small>
                    ) : (
                      !row.track.available && (
                        <small className="availability" data-state="catalog">
                          <span
                            className="ring"
                            data-ring="catalog"
                            aria-hidden="true"
                          />
                          Unavailable on this device
                        </small>
                      )
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
                    (track?.album ?? "—")
                  )}
                </td>
                <td className="duration-column muted">
                  {track ? time(track.duration_ms / 1000) : "—"}
                </td>
                <td>
                  {track ? (
                    <TrackActions
                      track={track}
                      playlists={playlists}
                      playlist={playlist}
                      editing={edit?.active ?? false}
                      mutate={mutate}
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
