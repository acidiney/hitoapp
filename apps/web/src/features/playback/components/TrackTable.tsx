import type { MouseEvent } from "react";
import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";
import { time } from "../../../shared/format";
import type { PlaybackStart, Track } from "../model";

import type { Mutate } from "../../../shared/hooks/useMutation";
import {
  PHONE_QUERY,
  useMediaQuery,
} from "../../../shared/hooks/useMediaQuery";
import TrackRowActions from "./TrackRowActions";
import PlaylistEntryRemove from "./PlaylistEntryRemove";
import EntryMoves from "./EntryMoves";
import RowPlay from "./RowPlay";
import LongPressRow from "./LongPressRow";
import TrackAvailabilityNote from "./TrackAvailabilityNote";
import TrackRowSource from "./TrackRowSource";
import TrackNames, { artistHref, albumHref } from "./TrackNames";
import { trackHref } from "./TrackRowMenu";
import Artwork from "../../artwork/components/Artwork";
import type { TrackListing } from "../api/trackListing";
import { usePlayTracks } from "../hooks/usePlayTracks";
import { PlayingBars, PlayingStatus } from "./PlayingBars";
import { rowPlayState, useNowPlaying } from "../hooks/useNowPlaying";

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

export type TrackTableRow =
  | { kind: "recording"; track: TrackRow }
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

export type RowSelection = {
  selected: ReadonlySet<string>;
  toggle(id: string): void;
};

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
  without = [],
  artistPage,
  layout,
  selection,
}: {
  layout?: "library" | "playlist";
  selection?: RowSelection;
  edit?: PlaylistEdit;
  rows: TrackTableRow[];
  listing?: TrackListing;
  offset?: number;
  playlists: Schema<"Playlist">[];
  playlist?: Schema<"Playlist">;
  mutate: Mutate;
  without?: TrackColumn[];
  artistPage?: string;
}) {
  const nowPlaying = useNowPlaying();
  const phone = useMediaQuery(PHONE_QUERY);
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
  const rowTapPlays =
    phone && !edit?.active && !(selection && selection.selected.size > 0);
  function tapRow(event: MouseEvent<HTMLElement>, start: (() => void) | null) {
    if (!rowTapPlays || !start) return;
    const target = event.target as Element;
    if (target.closest("a, button, input, label, [role='menu']")) return;
    start();
  }
  const anyAlbum = rows.some(
    (row) => row.kind === "recording" && row.track.album != null,
  );
  const creditedElsewhere = (row: TrackTableRow) =>
    artistPage === undefined ||
    row.kind !== "recording" ||
    row.track.artist_id !== artistPage;
  const showArtist =
    !layout && !without.includes("artist") && rows.some(creditedElsewhere);
  const showAlbum =
    !without.includes("album") && anyAlbum && !(layout && edit?.active);
  return (
    <div className="table-scroll">
      <table
        className="track-table"
        data-editing={edit?.active || undefined}
        data-layout={layout}
        data-selecting={(selection && selection.selected.size > 0) || undefined}
        aria-label={playlist ? "Playlist entries" : "Tracks"}
      >
        <thead>
          <tr>
            <th className="number-column">
              {selection ? (
                "Select"
              ) : (
                <>
                  <span className="sr-only">Play</span>#
                </>
              )}
            </th>
            <th>Title</th>
            {showArtist && <th className="artist-column">Artist</th>}
            {showAlbum && <th className="album-column">Album</th>}
            {layout && <th className="source-column">Availability</th>}
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
            const album = track?.album ?? null;
            const albumLink = track ? albumHref(track) : null;
            const artistLink = track ? artistHref(track) : null;
            const duration = track !== null ? track.duration_ms : null;
            const playState = rowPlayState(nowPlaying, track);
            return (
              <LongPressRow
                key={rowId}
                onLongPress={
                  selection && track ? () => selection.toggle(rowId) : undefined
                }
                onClick={(event) =>
                  tapRow(event, track ? () => play(track) : null)
                }
                aria-current={playState ? "true" : undefined}
                className={[
                  playState ? "playing-row" : "",
                  edit?.removed.has(rowId) ? "removed-row" : "",
                  selection?.selected.has(rowId) ? "selected-row" : "",
                ]
                  .filter(Boolean)
                  .join(" ")}
              >
                <td className="number-column">
                  {selection && track ? (
                    <label className="row-select">
                      <input
                        type="checkbox"
                        checked={selection.selected.has(rowId)}
                        aria-label={`Select ${track.title}`}
                        onChange={() => selection.toggle(rowId)}
                      />
                    </label>
                  ) : track ? (
                    <RowPlay
                      title={track.title}
                      number={trackNumber(track, index + 1 + offset)}
                      state={playState}
                      onPlay={() => play(track)}
                    />
                  ) : (
                    <span className="row-number">{index + 1 + offset}</span>
                  )}
                </td>
                <td className="track-cell">
                  {selection && playState && (
                    <span className="cover-bars">
                      <PlayingBars state={playState} />
                      <PlayingStatus state={playState} />
                    </span>
                  )}
                  {selection && track && (
                    <span className="cover-play">
                      <RowPlay
                        title={track.title}
                        number=""
                        onPlay={() => play(track)}
                      />
                    </span>
                  )}
                  {track ? (
                    <Artwork
                      release={
                        layout === "library"
                          ? undefined
                          : (track.release_id ?? undefined)
                      }
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
                    ) : (
                      <span className="retained-title">{display.title}</span>
                    )}
                    <div className="track-sub">
                      {row.kind === "retained" ? (
                        <small>{retainedNote(row.entry.origin)}</small>
                      ) : layout ? null : (
                        <TrackAvailabilityNote track={track!} />
                      )}
                      <TrackNames
                        className="track-meta"
                        artist={
                          (showArtist || layout) && creditedElsewhere(row)
                            ? display.artist
                            : undefined
                        }
                        artistHref={artistLink}
                        credited={track?.credited_artists}
                        album={showAlbum && !layout ? album : null}
                        albumHref={albumLink}
                      />
                    </div>
                  </div>
                </td>
                {showArtist && (
                  <td className="artist-column muted">
                    {creditedElsewhere(row) && (
                      <TrackNames
                        artist={display.artist}
                        artistHref={artistLink}
                        credited={track?.credited_artists}
                      />
                    )}
                  </td>
                )}
                {showAlbum && (
                  <td className="album-column muted">
                    <TrackNames album={album} albumHref={albumLink} />
                  </td>
                )}
                {layout && (
                  <td className="source-column">
                    <TrackRowSource
                      track={row.kind === "retained" ? null : track}
                    />
                  </td>
                )}
                <td className="duration-column muted">
                  {duration == null ? "—" : time(duration / 1000)}
                </td>
                <td className="actions-column">
                  {track ? (
                    <TrackRowActions
                      track={track}
                      playlists={playlists}
                      playlist={playlist}
                      mutate={mutate}
                      liking={!edit?.active}
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
              </LongPressRow>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
