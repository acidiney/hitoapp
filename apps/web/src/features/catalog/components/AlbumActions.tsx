import { router } from "@inertiajs/react";
import { Play, Plus, Shuffle } from "lucide-react";
import ActionMenu, { type ActionMenuItem } from "../../../shared/ui/ActionMenu";
import Button from "../../../shared/ui/Button";
import {
  PHONE_QUERY,
  useMediaQuery,
} from "../../../shared/hooks/useMediaQuery";
import type { AlbumAdditionState } from "../hooks/useAlbumAddition";

export type AlbumPlay = {
  disabled: boolean;
  play: () => void;
  shuffle: () => void;
};

function addLabel(addition: AlbumAdditionState) {
  return addition.missing === addition.total
    ? "Add album"
    : `Add the other ${addition.missing}`;
}

function tracks(count: number) {
  return `${count} ${count === 1 ? "track" : "tracks"}`;
}

export default function AlbumActions({
  play,
  addition = null,
  detailsHref,
  retry,
}: {
  play: AlbumPlay;
  addition?: AlbumAdditionState | null;
  detailsHref: string | null;
  retry: ActionMenuItem | null;
}) {
  const phone = useMediaQuery(PHONE_QUERY);
  const canAdd = addition !== null && addition.missing > 0 && !addition.added;
  const items: ActionMenuItem[] = [
    ...(phone
      ? [
          {
            label: "Shuffle album",
            disabled: play.disabled,
            onSelect: play.shuffle,
          },
        ]
      : []),
    ...(phone && canAdd
      ? [
          {
            label: addLabel(addition),
            disabled: addition.phase !== null,
            onSelect: () => void addition.add(),
          },
        ]
      : []),
    ...(detailsHref
      ? [
          {
            label: "Edit album details",
            onSelect: () => router.visit(detailsHref),
          },
        ]
      : []),
    ...(retry ? [retry] : []),
  ];
  return (
    <>
      <button
        type="button"
        className="pill-action"
        data-primary=""
        aria-label="Play album"
        disabled={play.disabled}
        onClick={play.play}
      >
        <Play size={16} fill="currentColor" aria-hidden="true" />
        Play
      </button>
      {!phone && (
        <button
          type="button"
          className="pill-icon"
          aria-label="Shuffle album"
          disabled={play.disabled}
          onClick={play.shuffle}
        >
          <Shuffle size={18} aria-hidden="true" />
        </button>
      )}
      {!phone && canAdd && (
        <button
          type="button"
          className="pill-action"
          disabled={addition.phase !== null}
          onClick={() => void addition.add()}
        >
          <Plus size={16} aria-hidden="true" />
          {addLabel(addition)}
        </button>
      )}
      {items.length > 0 && (
        <ActionMenu
          label="More actions"
          triggerClassName="pill-icon"
          align="end"
          items={items}
        />
      )}
      {addition?.added && (
        <p className="album-addition" role="status">
          Added {tracks(addition.added.added)} to your library.
          <Button
            disabled={addition.phase !== null}
            onClick={() => void addition.undo()}
          >
            Undo
          </Button>
        </p>
      )}
      {addition?.undone && (
        <p className="album-addition" role="status">
          {addition.undone.kept > 0
            ? `Removed ${tracks(addition.undone.removed)}. Kept ${tracks(addition.undone.kept)} you've already used.`
            : `Removed ${tracks(addition.undone.removed)}.`}
        </p>
      )}
      {addition?.error && (
        <p className="album-addition" role="alert">
          {addition.error}
        </p>
      )}
    </>
  );
}
