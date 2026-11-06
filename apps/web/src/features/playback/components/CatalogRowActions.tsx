import { Link } from "@inertiajs/react";
import { Search } from "lucide-react";
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
}: {
  title: string;
  status: CatalogRowView["status"];
  onFind: () => void;
}) {
  const retry = status === "not_found" || status === "failed";
  return (
    <div className="track-actions">
      {retry && (
        <Link
          className="text-link"
          href="/settings?section=music"
          aria-label={`Connect services to find ${title}`}
        >
          Services
        </Link>
      )}
      <button
        type="button"
        className="icon-button"
        disabled={status === "finding"}
        aria-label={`${retry ? "Try again to find" : "Find"} audio for ${title}`}
        onClick={onFind}
      >
        <Search size={17} />
      </button>
    </div>
  );
}
