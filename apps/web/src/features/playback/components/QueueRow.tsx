import { memo } from "react";
import { ChevronDown, ChevronUp, GripVertical, X } from "lucide-react";
import Artwork from "../../artwork/components/Artwork";
import { coverIdentity } from "../../artwork/model/coverIdentity";
import type { PlaybackOccurrence } from "../model";
import { QUEUE_ROW } from "../hooks/useQueueWindow";
import PlayerTrackActions from "./PlayerTrackActions";

type Props = {
  entry: PlaybackOccurrence;
  index: number;
  first: number;
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
  first,
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
      data-entry={id}
      data-drop={marker ?? undefined}
      aria-posinset={index - first + 1}
      aria-setsize={total - first}
      style={{ transform: `translateY(${(index - first) * QUEUE_ROW}px)` }}
    >
      <button
        className="queue-handle"
        draggable
        aria-label={`Drag ${track.title}`}
        title="Drag to reorder"
      >
        <GripVertical size={18} strokeWidth={3} />
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
      <PlayerTrackActions track={track} liking={false} />
      <span className="queue-order">
        <button
          className="icon-button"
          data-move="up"
          aria-label={`Move ${track.title} up`}
          disabled={index === first}
          onClick={() => move(id, index, -1)}
        >
          <ChevronUp size={14} />
        </button>
        <button
          className="icon-button"
          data-move="down"
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
