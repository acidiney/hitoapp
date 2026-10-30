import type { Schema } from "../../../shared/schema";

import { Link } from "@inertiajs/react";
import Button from "../../../shared/ui/Button";

export default function ArtistIdentityChoice({
  choice,
  name,
  pending,
  error,
  canChoose,
  canReset,
  retrying,
  onChoose,
  onReset,
  onRetry,
  onRefresh,
}: {
  choice: Schema<"RecordingArtistIdentityChoice"> | null;
  name: string | null;
  pending: string | null;
  error: string;
  canChoose: boolean;
  canReset: boolean;
  retrying: boolean;
  onChoose(): void;
  onReset(): void;
  onRetry(): void;
  onRefresh(): void;
}) {
  return (
    <section
      className="rd-panel"
      aria-label="Artist choice for this track"
      aria-busy={pending !== null}
    >
      <h3>Artist choice for this track</h3>
      {choice?.state.status === "unselected" && (
        <p>No reviewed artist selected for this track.</p>
      )}
      {choice?.state.status === "selected" && (
        <p role="status">Reviewed artist selected for this track.</p>
      )}
      {choice?.state.status === "needs_review" && (
        <p>This artist choice needs review.</p>
      )}
      {name !== null && <p>Saved credit for this group: {name}</p>}
      {choice?.state.status === "selected" &&
        name !== null &&
        pending === null &&
        !retrying && (
          <Link
            className="text-link"
            href={`/artist-groups/${encodeURIComponent(choice.state.group_id)}`}
          >
            Open chosen artist
          </Link>
        )}
      {error && <p role="alert">{error}</p>}
      {retrying && (
        <Button type="button" disabled={pending !== null} onClick={onRetry}>
          Retry the same artist choice request
        </Button>
      )}
      {canChoose && (
        <Button type="button" onClick={onChoose}>
          Choose this reviewed artist
        </Button>
      )}
      {canReset && (
        <Button type="button" onClick={onReset}>
          Reset artist choice for this track
        </Button>
      )}
      {!choice && pending === null && (
        <Button type="button" onClick={onRefresh}>
          Check current artist choice
        </Button>
      )}
    </section>
  );
}
