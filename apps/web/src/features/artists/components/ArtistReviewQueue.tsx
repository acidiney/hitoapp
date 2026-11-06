import { Link } from "@inertiajs/react";
import { useRef, useState } from "react";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import { useArtistReviewQueue } from "../hooks/useArtistReviewQueue";
import {
  candidateDetails,
  candidateLabel,
  plural,
} from "../model/reviewCandidateText";
import { artistSearchHref } from "../model/artistSearchLink";
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
  const [open, setOpen] = useState(false);
  const opener = useRef<HTMLButtonElement>(null);
  const waiting = queue?.total ?? 0;
  const dismissedTotal = dismissed?.total ?? 0;
  const label =
    waiting > 0
      ? `${waiting} ${waiting === 1 ? "artist needs" : "artists need"} review`
      : `Dismissed suggestions (${dismissedTotal})`;
  return (
    <>
      {queue && waiting + dismissedTotal > 0 && (
        <Button ref={opener} onClick={() => setOpen(true)}>
          {label}
        </Button>
      )}
      {queue && waiting + dismissedTotal > 0 && open && (
        <Dialog
          wide
          title="Artists to review"
          description="Hito could not decide these credits on its own. Confirming an artist applies it to the tracks of that credit that have no artist yet."
          busy={pending !== null}
          returnFocus={opener}
          onClose={() => setOpen(false)}
        >
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
                  {item.needs_search && (
                    <div className="review-queue-nomatch">
                      <span>No match found automatically</span>
                      {item.search_recording_id && (
                        <Link
                          className="button secondary"
                          href={artistSearchHref(item.search_recording_id)}
                          aria-label={`Search for ${item.name}`}
                        >
                          Search
                        </Link>
                      )}
                    </div>
                  )}
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
                  {!item.needs_search && (
                    <Button
                      disabled={pending !== null}
                      aria-label={`None of these for ${item.name}`}
                      onClick={() => void dismiss(item.artist_id, null)}
                    >
                      None of these
                    </Button>
                  )}
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
          {dismissed && dismissed.total > 0 && (
            <ArtistDismissedReviews
              dismissed={dismissed}
              pending={pending}
              error={restoreError}
              restore={restore}
              more={() => void more("dismissed")}
            />
          )}
          <footer className="review-actions">
            <Button onClick={() => setOpen(false)}>Close</Button>
          </footer>
        </Dialog>
      )}
      {queue && dismissedFailed && (
        <p className="muted">Dismissed suggestions could not be loaded.</p>
      )}
    </>
  );
}
