import { useId } from "react";
import type { Schema } from "../../../shared/schema";
import Button from "../../../shared/ui/Button";
import {
  candidateDetails,
  candidateLabel,
  plural,
} from "../model/reviewCandidateText";

export default function ArtistDismissedReviews({
  dismissed,
  pending,
  error,
  restore,
  more,
}: {
  dismissed: Schema<"ArtistReviewQueuePage">;
  pending: string | null;
  error: string;
  restore(credit: string, artist: string | null): Promise<void>;
  more(): void;
}) {
  const titleId = useId();
  return (
    <section className="review-queue" aria-labelledby={titleId}>
      <details>
        <summary>
          <h2 className="discography-heading" id={titleId}>
            Dismissed suggestions ({dismissed.total})
          </h2>
        </summary>
        <p className="muted">
          You said these were not the right artist. Restoring offers them again
          for confirmation.
        </p>
        {error && (
          <p className="review-queue-error" role="alert">
            {error}
          </p>
        )}
        <ul className="review-queue-list">
          {dismissed.items.map((item) => (
            <li
              className="review-queue-row"
              key={item.artist_id}
              aria-busy={pending === item.artist_id}
            >
              <div className="review-queue-credit">
                <strong>{item.name}</strong>
                <small>
                  {plural(item.undecided_tracks, "track", "tracks")}
                </small>
              </div>
              <ul className="review-queue-candidates">
                {item.candidates.map((candidate) => (
                  <li
                    className="review-queue-candidate"
                    key={candidate.provider_artist_id}
                  >
                    <span>{candidateDetails(candidate) || candidate.name}</span>
                    <Button
                      disabled={pending !== null}
                      aria-label={`Restore ${candidateLabel(candidate)}, for ${item.name}`}
                      onClick={() =>
                        void restore(
                          item.artist_id,
                          candidate.provider_artist_id,
                        )
                      }
                    >
                      Restore
                    </Button>
                  </li>
                ))}
              </ul>
              {item.candidates.length > 1 && (
                <Button
                  disabled={pending !== null}
                  aria-label={`Restore all for ${item.name}`}
                  onClick={() => void restore(item.artist_id, null)}
                >
                  Restore all
                </Button>
              )}
            </li>
          ))}
        </ul>
        {dismissed.items.length < dismissed.total && (
          <Button
            disabled={pending !== null}
            aria-label="Show more dismissed"
            onClick={more}
          >
            Show more
          </Button>
        )}
      </details>
    </section>
  );
}
