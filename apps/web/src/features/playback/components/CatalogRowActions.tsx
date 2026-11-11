import { router } from "@inertiajs/react";
import { Heart, Search } from "lucide-react";
import ActionMenu from "../../../shared/ui/ActionMenu";
import StatusDot, { type SourceStatus } from "../../../shared/ui/StatusDot";
import type { CatalogRowView } from "./TrackTable";

const statuses: Partial<Record<CatalogRowView["status"], SourceStatus>> = {
  finding: "checking",
  not_found: "none",
  failed: "failed",
};

export function CatalogRowNote({
  status,
}: {
  status: CatalogRowView["status"];
}) {
  const shown = statuses[status];
  return shown ? <StatusDot status={shown} as="small" role="status" /> : null;
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
