import { memo, type PointerEvent } from "react";
import Artwork from "../../artwork/components/Artwork";
import { coverIdentity } from "../../artwork/model/coverIdentity";
import type { PlaybackOccurrence } from "../model";
import { QUEUE_ROW } from "../hooks/useQueueWindow";
import { GripIcon } from "./QueueIcons";
import QueueRowMenu from "./QueueRowMenu";

type Props = {
  entry: PlaybackOccurrence;
  index: number;
  first: number;
  total: number;
  current: boolean;
  lifted: boolean;
  shift: number;
  select(entry: string): void;
  remove(entry: string): void;
  move(entry: string, index: number, step: -1 | 1): void;
  grab(
    event: PointerEvent<HTMLElement>,
    entry: PlaybackOccurrence,
    index: number,
  ): void;
};

function optionKey(): string {
  const platform =
    typeof navigator === "undefined"
      ? ""
      : navigator.platform || navigator.userAgent;
  return /Mac|iPhone|iPad/i.test(platform) ? "Option" : "Alt";
}

const QueueRow = memo(function QueueRow({
  entry,
  index,
  first,
  total,
  current,
  lifted,
  shift,
  select,
  remove,
  move,
  grab,
}: Props) {
  const { id, recording: track } = entry;
  return (
    <li
      className={current ? "current" : ""}
      data-index={index}
      data-entry={id}
      data-lifted={lifted ? "" : undefined}
      aria-posinset={index - first + 1}
      aria-setsize={total - first}
      style={{
        transform: `translateY(calc(${(index - first) * QUEUE_ROW + shift}px + var(--drag-y, 0px)))`,
      }}
    >
      <button
        type="button"
        className="queue-handle"
        aria-label={`Reorder ${track.title}: drag, or press ${optionKey()} and Up or Down`}
        onPointerDown={(event) => grab(event, entry, index)}
      >
        <GripIcon />
      </button>
      <button className="queue-track" onClick={() => select(id)}>
        <Artwork
          release={track.release_id ?? undefined}
          recording={track.id}
          fallback={coverIdentity(track)}
        />
        <span>
          <strong>{track.title}</strong>
          <small>{track.artist}</small>
        </span>
      </button>
      <QueueRowMenu
        track={track}
        canMoveUp={index > first}
        canMoveDown={index < total - 1}
        moveUp={() => move(id, index, -1)}
        moveDown={() => move(id, index, 1)}
        remove={() => remove(id)}
      />
    </li>
  );
});

export default QueueRow;
