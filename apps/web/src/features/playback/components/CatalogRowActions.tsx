import { router } from "@inertiajs/react";
import { Plus } from "lucide-react";
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
  if (shown) return <StatusDot status={shown} as="small" role="status" />;
  return (
    <small className="source-status" data-status="absent">
      <span className="source-status-dot" aria-hidden="true" />
      Not in your library
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
        aria-label={`${retry ? "Try again to find" : "Find"} audio for ${title}`}
        onClick={onFind}
      >
        <Plus size={17} />
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
          {
            label: "Find audio and like",
            disabled: finding,
            onSelect: onLike,
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
