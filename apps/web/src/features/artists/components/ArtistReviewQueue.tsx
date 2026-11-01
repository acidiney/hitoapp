import { useId } from "react";
import Button from "../../../shared/ui/Button";
import { useArtistReviewQueue } from "../hooks/useArtistReviewQueue";
import {
  candidateDetails,
  candidateLabel,
  plural,
} from "../model/reviewCandidateText";
import ArtistDismissedReviews from "./ArtistDismissedReviews";

export default function ArtistReviewQueue({
  account,
  refreshKey,
}: {
  account: string;
  refreshKey: string;
}) {
  const {
    queue,
    dismissed,
    dismissedFailed,
    pending,
    error,
    restoreError,
    confirm,
    dismiss,
    restore,
    more,
  } = useArtistReviewQueue(account, refreshKey);
  const titleId = useId();
  return (
    <>
      {queue && queue.total > 0 && (
        <section className="review-queue" aria-labelledby={titleId}>
          <details>
            <summary>
              <h2 className="discography-heading" id={titleId}>
                Needs your confirmation ({queue.total})
              </h2>
            </summary>
            <p className="muted">
              Hito could not tell these artists apart on its own. Confirming
              applies the artist to the undecided tracks of that credit.
            </p>
            {error && (
              <p className="review-queue-error" role="alert">
                {error}
              </p>
            )}
            <ul className="review-queue-list">
              {queue.items.map((item) => {
                const busy = pending === item.artist_id;
                return (
                  <li
                    className="review-queue-row"
                    key={item.artist_id}
                    aria-busy={busy}
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
                          <span>
                            {candidateDetails(candidate) || candidate.name}
                          </span>
                          <Button
                            disabled={pending !== null}
                            aria-label={`Confirm ${candidateLabel(candidate)}, for ${item.name}`}
                            onClick={() =>
                              void confirm(
                                item.artist_id,
                                candidate.provider_artist_id,
                              )
                            }
                          >
                            Confirm
                          </Button>
                        </li>
                      ))}
                    </ul>
                    <Button
                      disabled={pending !== null}
                      aria-label={`None of these for ${item.name}`}
                      onClick={() => void dismiss(item.artist_id, null)}
                    >
                      None of these
                    </Button>
                  </li>
                );
              })}
            </ul>
            {queue.items.length < queue.total && (
              <Button
                disabled={pending !== null}
                onClick={() => void more("queue")}
              >
                Show more
              </Button>
            )}
          </details>
        </section>
      )}
      {queue && dismissedFailed && (
        <p className="muted">Dismissed suggestions could not be loaded.</p>
      )}
      {dismissed && dismissed.total > 0 && (
        <ArtistDismissedReviews
          dismissed={dismissed}
          pending={pending}
          error={restoreError}
          restore={restore}
          more={() => void more("dismissed")}
        />
      )}
    </>
  );
}
