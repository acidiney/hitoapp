import { ArrowDown, ArrowUp } from "lucide-react";
import type { PlaylistEdit } from "./TrackTable";

export default function EntryMoves({
  entryId,
  title,
  first,
  last,
  removed,
  edit,
}: {
  entryId: string;
  title: string;
  first: boolean;
  last: boolean;
  removed: boolean;
  edit: PlaylistEdit;
}) {
  return (
    <>
      <button
        type="button"
        className="icon-button"
        aria-label={`Move ${title} up`}
        disabled={first || edit.busy || removed}
        onClick={() => edit.move(entryId, -1)}
      >
        <ArrowUp size={16} />
      </button>
      <button
        type="button"
        className="icon-button"
        aria-label={`Move ${title} down`}
        disabled={last || edit.busy || removed}
        onClick={() => edit.move(entryId, 1)}
      >
        <ArrowDown size={16} />
      </button>
    </>
  );
}
