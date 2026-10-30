import type { Schema } from "../../../shared/schema";

import Button from "../../../shared/ui/Button";
import { associationEvidenceText } from "../model/associationEvidence";

export default function SavedArtistReviews({
  associations,
  disabled,
  ready,
  canUndo,
  retrying,
  error,
  onView,
  onUnlink,
  onUndo,
  onRetry,
}: {
  associations: Schema<"ArtistAssociation">[];
  disabled: boolean;
  ready: boolean;
  canUndo: boolean;
  retrying: boolean;
  error: string;
  onView(association: Schema<"ArtistAssociation">): void;
  onUnlink(association: Schema<"ArtistAssociation">): void;
  onUndo(): void;
  onRetry(): void;
}) {
  if (associations.length === 0 && !canUndo && !retrying && !error) return null;
  return (
    <section aria-label="Saved artist reviews">
      <h3>Saved artist reviews</h3>
      <ul>
        {associations.map((association, index) => (
          <li key={association.id}>
            {associationEvidenceText(association.evidence) && (
              <p>{associationEvidenceText(association.evidence)}</p>
            )}
            <Button
              type="button"
              disabled={disabled}
              onClick={() => onView(association)}
            >
              View saved artist review {index + 1}
            </Button>
            <Button
              type="button"
              disabled={disabled || !ready || retrying}
              onClick={() => onUnlink(association)}
            >
              Unlink saved artist review {index + 1}
            </Button>
          </li>
        ))}
      </ul>
      {error && <p role="alert">{error}</p>}
      {retrying && (
        <Button type="button" disabled={disabled} onClick={onRetry}>
          Retry the same artist review change request
        </Button>
      )}
      {canUndo && (
        <Button type="button" disabled={disabled} onClick={onUndo}>
          Undo last artist review change
        </Button>
      )}
    </section>
  );
}
