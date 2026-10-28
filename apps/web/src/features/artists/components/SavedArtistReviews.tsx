import type { ArtistAssociation } from "../../../shared/types";

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
  associations: ArtistAssociation[];
  disabled: boolean;
  ready: boolean;
  canUndo: boolean;
  retrying: boolean;
  error: string;
  onView(association: ArtistAssociation): void;
  onUnlink(association: ArtistAssociation): void;
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
            <button
              type="button"
              className="button"
              disabled={disabled}
              onClick={() => onView(association)}
            >
              View saved artist review {index + 1}
            </button>
            <button
              type="button"
              className="button"
              disabled={disabled || !ready || retrying}
              onClick={() => onUnlink(association)}
            >
              Unlink saved artist review {index + 1}
            </button>
          </li>
        ))}
      </ul>
      {error && <p role="alert">{error}</p>}
      {retrying && (
        <button
          type="button"
          className="button"
          disabled={disabled}
          onClick={onRetry}
        >
          Retry the same artist review change request
        </button>
      )}
      {canUndo && (
        <button
          type="button"
          className="button"
          disabled={disabled}
          onClick={onUndo}
        >
          Undo last artist review change
        </button>
      )}
    </section>
  );
}
