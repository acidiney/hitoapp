import { memo } from "react";
import { ChevronDown, ChevronUp, GripVertical, X } from "lucide-react";
import Artwork from "../../artwork/components/Artwork";
import { time } from "../../../shared/format";
import type { PlaybackOccurrence } from "../model";
import { QUEUE_ROW } from "../hooks/useQueueWindow";

type Props = {
  entry: PlaybackOccurrence;
  index: number;
  total: number;
  current: boolean;
  marker: "before" | "after" | null;
  select(entry: string): void;
  remove(entry: string): void;
  move(entry: string, index: number, step: -1 | 1): void;
};

const QueueRow = memo(function QueueRow({
  entry,
  index,
  total,
  current,
  marker,
  select,
  remove,
  move,
}: Props) {
  const { id, recording: track } = entry;
  return (
    <li
      className={current ? "current" : ""}
      data-index={index}
      data-drop={marker ?? undefined}
      aria-posinset={index + 1}
      aria-setsize={total}
      style={{ transform: `translateY(${index * QUEUE_ROW}px)` }}
    >
      <button
        className="queue-handle"
        draggable
        aria-label={`Drag ${track.title}`}
        title="Drag to reorder"
      >
        <GripVertical size={16} />
      </button>
      <button className="queue-track" onClick={() => select(id)}>
        <Artwork release={track.release_id ?? undefined} recording={track.id} />
        <span>
          <strong>{track.title}</strong>
          <small>{track.artist}</small>
        </span>
      </button>
      <span className="queue-duration">{time(track.duration_ms / 1000)}</span>
      <span className="queue-order">
        <button
          className="icon-button"
          aria-label={`Move ${track.title} up`}
          disabled={index === 0}
          onClick={() => move(id, index, -1)}
        >
          <ChevronUp size={14} />
        </button>
        <button
          className="icon-button"
          aria-label={`Move ${track.title} down`}
          disabled={index === total - 1}
          onClick={() => move(id, index, 1)}
        >
          <ChevronDown size={14} />
        </button>
      </span>
      <button
        className="icon-button"
        aria-label={`Remove ${track.title} from queue`}
        onClick={() => remove(id)}
      >
        <X size={16} />
      </button>
    </li>
  );
});

export default QueueRow;
