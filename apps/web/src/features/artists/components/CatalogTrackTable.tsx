import { useSyncExternalStore } from "react";
import { time } from "../../../shared/format";
import { usePlayer } from "../../playback/components/PlaybackProvider";
import RowPlay from "../../playback/components/RowPlay";
import type { CatalogRow } from "../model/catalogAlbumRows";
import type { RowState } from "../model/catalogQueue";

const notes: Partial<Record<RowState["status"], string>> = {
  finding: "Finding audio…",
  not_found: "Not found on your services",
  failed: "Couldn't search right now",
};

export default function CatalogTrackTable({
  rows,
  states,
  onPlay,
}: {
  rows: CatalogRow[];
  states: ReadonlyMap<string, RowState>;
  onPlay: (index: number) => void;
}) {
  const player = usePlayer();
  const currentId = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().current?.id,
  );
  return (
    <div className="table-scroll">
      <table className="track-table" aria-label="Tracks">
        <thead>
          <tr>
            <th className="number-column">
              <span className="sr-only">Play</span>#
            </th>
            <th>Title</th>
            <th className="duration-column">Time</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row, index) => {
            const state = states.get(row.id) ?? { status: "idle" };
            const playing =
              state.status === "found" && state.recording === currentId;
            return (
              <tr key={row.id} className={playing ? "playing-row" : ""}>
                <td>
                  <RowPlay
                    title={row.title}
                    number={row.number}
                    onPlay={() => onPlay(index)}
                  />
                </td>
                <td className="track-cell">
                  <div>
                    <button
                      className="track-title"
                      onClick={() => onPlay(index)}
                    >
                      {row.title}
                    </button>
                    {row.credit && <span className="muted">{row.credit}</span>}
                    {notes[state.status] && (
                      <small role="status">{notes[state.status]}</small>
                    )}
                  </div>
                </td>
                <td className="duration-column muted">
                  {row.durationMs == null ? "—" : time(row.durationMs / 1000)}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
