import { router } from "@inertiajs/react";
import { Heart, Search } from "lucide-react";
import ActionMenu from "../../../shared/ui/ActionMenu";
import type { CatalogRowView } from "./TrackTable";

const notes: Partial<Record<CatalogRowView["status"], string>> = {
  finding: "Finding audio…",
  not_found: "Not found on your services",
  failed: "Couldn't search right now",
};

export function CatalogRowNote({
  status,
}: {
  status: CatalogRowView["status"];
}) {
  const note = notes[status];
  if (!note) return null;
  return (
    <small role="status" className="availability" data-state="catalog">
      <span className="ring" data-ring="catalog" aria-hidden="true" />
      {note}
    </small>
  );
}

export default function CatalogRowActions({
  title,
  status,
  onFind,
  onOpen,
  onLike,
}: {
  onOpen: () => void;
  onLike: () => void;
  title: string;
  status: CatalogRowView["status"];
  onFind: () => void;
}) {
  const retry = status === "not_found" || status === "failed";
  const finding = status === "finding";
  return (
    <div className="track-actions">
      <button
        type="button"
        className="icon-button"
        disabled={finding}
        aria-label={`Find audio and like ${title}`}
        onClick={onLike}
      >
        <Heart size={17} />
      </button>
      <button
        type="button"
        className="icon-button"
        disabled={finding}
        aria-label={`${retry ? "Try again to find" : "Find"} audio for ${title}`}
        onClick={onFind}
      >
        <Search size={17} />
      </button>
      <ActionMenu
        label={`Actions for ${title}`}
        triggerClassName="icon-button"
        align="end"
        items={[
          {
            label: "Track details",
            disabled: finding,
            onSelect: onOpen,
          },
          {
            label: retry ? "Try again to find audio" : "Find audio",
            disabled: finding,
            onSelect: onFind,
          },
          ...(retry
            ? [
                {
                  label: "Connect services",
                  onSelect: () => router.visit("/settings?section=music"),
                },
              ]
            : []),
        ]}
      />
    </div>
  );
}
