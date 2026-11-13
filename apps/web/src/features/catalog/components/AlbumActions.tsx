import { router } from "@inertiajs/react";
import { Play, Shuffle } from "lucide-react";
import ActionMenu, { type ActionMenuItem } from "../../../shared/ui/ActionMenu";
import {
  PHONE_QUERY,
  useMediaQuery,
} from "../../../shared/hooks/useMediaQuery";

export type AlbumPlay = {
  disabled: boolean;
  play: () => void;
  shuffle: () => void;
};

export default function AlbumActions({
  play,
  detailsHref,
  retry,
}: {
  play: AlbumPlay;
  detailsHref: string | null;
  retry: ActionMenuItem | null;
}) {
  const phone = useMediaQuery(PHONE_QUERY);
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
      {items.length > 0 && (
        <ActionMenu
          label="More actions"
          triggerClassName="pill-icon"
          align="end"
          items={items}
        />
      )}
    </>
  );
}
