import type { Schema } from "../../../shared/schema";

import Button from "../../../shared/ui/Button";
import {
  associationEvidenceText,
  providerName,
} from "../model/associationEvidence";

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
    <section className="rd-panel" aria-label="Saved artist reviews">
      <h3>Saved artist reviews</h3>
      <ul className="rd-rows">
        {associations.map((association, index) => (
          <li className="rd-row" key={association.id}>
            <div className="rd-row-copy">
              <strong>
                {providerName(association.provider)} · Recording artist
              </strong>
              <p>
                {associationEvidenceText(association.evidence) ??
                  "Confirmed by you."}
              </p>
            </div>
            <div className="rd-row-actions">
              <Button
                type="button"
                className="compact"
                aria-label={`View saved artist review ${index + 1}`}
                disabled={disabled}
                onClick={() => onView(association)}
              >
                View
              </Button>
              <Button
                type="button"
                className="compact"
                aria-label={`Unlink saved artist review ${index + 1}`}
                disabled={disabled || !ready || retrying}
                onClick={() => onUnlink(association)}
              >
                Unlink
              </Button>
            </div>
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
