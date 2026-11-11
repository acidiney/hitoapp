import type { Schema } from "../../../shared/schema";
import ActionMenu from "../../../shared/ui/ActionMenu";
import { useMutation } from "../../../shared/hooks/useMutation";
import { useNoticeIfProvided } from "../../../shared/ui/Notices";
import { createUuid } from "../../../shared/uuid";
import { log } from "../../../shared/log";
import { usePlayer } from "../../playback/components/PlaybackProvider";
import type { TrackRow } from "../../playback/components/TrackTable";
import type { TrackSelection } from "../hooks/useTrackSelection";

export default function SelectionBar({
  account,
  playlists,
  tracks,
  selection,
}: {
  account: string;
  playlists: Schema<"Playlist">[];
  tracks: TrackRow[];
  selection: TrackSelection;
}) {
  const player = usePlayer();
  const notify = useNoticeIfProvided();
  const { mutateMany } = useMutation(account);
  const chosen = tracks.filter((track) => selection.selected.has(track.row_id));
  const editable = playlists.filter((list) => list.kind === "local");
  if (chosen.length === 0) return null;

  async function queue() {
    log.info("library.selection_queue", { count: chosen.length });
    for (const track of chosen) await player.addToQueue?.(track);
    notify(`Added ${chosen.length} to the queue`);
    selection.clear();
  }

  async function addTo(list: Schema<"Playlist">) {
    log.info("library.selection_playlist", { count: chosen.length });
    const start = Date.now();
    const result = await mutateMany(
      chosen.map((track, index) => ({
        kind: "entry_insert",
        playlist_id: list.id,
        entry_id: createUuid(),
        recording_id: track.id,
        position: start + index,
      })),
    );
    if (!result.ok) return;
    notify(`Added ${chosen.length} to ${list.name}`);
    selection.clear();
  }

  return (
    <div
      role="toolbar"
      aria-label="Selection actions"
      className="selection-bar"
    >
      <span className="selection-count">{chosen.length} selected</span>
      {chosen.length < tracks.length && (
        <button
          type="button"
          className="selection-outline"
          onClick={() => selection.select(tracks.map((track) => track.row_id))}
        >
          Select the {tracks.length} loaded
        </button>
      )}
      <span className="selection-divider" aria-hidden="true" />
      {player.addToQueue && (
        <button type="button" onClick={() => void queue()}>
          Add to queue
        </button>
      )}
      <ActionMenu
        label="Add to playlist"
        triggerClassName="selection-menu"
        icon={<span>Add to playlist</span>}
        items={
          editable.length > 0
            ? editable.map((list) => ({
                label: list.name,
                onSelect: () => void addTo(list),
              }))
            : [{ label: "No playlists yet", disabled: true }]
        }
      />
      <span className="selection-spacer" />
      <button
        type="button"
        className="selection-clear"
        onClick={selection.clear}
      >
        Clear selection
      </button>
    </div>
  );
}
