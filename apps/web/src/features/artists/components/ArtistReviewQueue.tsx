import { useId } from "react";
import type { Schema } from "../../../shared/schema";
import Button from "../../../shared/ui/Button";
import { useArtistReviewQueue } from "../hooks/useArtistReviewQueue";

type Candidate = Schema<"ArtistReviewCandidate">;

function plural(count: number, one: string, many: string) {
  return `${count} ${count === 1 ? one : many}`;
}

/** What tells namesakes apart: the MusicBrainz note, type and country, then what the library shares. */
function details(candidate: Candidate) {
  const parts = [candidate.disambiguation, candidate.kind, candidate.country];
  if (candidate.corroborated_titles > 0)
    parts.push(
      `${plural(candidate.corroborated_titles, "title", "titles")} match your library`,
    );
  if (candidate.in_library) parts.push("already in your artists");
  return parts.filter(Boolean).join(" · ");
}

function label(candidate: Candidate) {
  return [candidate.name, candidate.disambiguation].filter(Boolean).join(", ");
}

export default function ArtistReviewQueue({
  account,
  refreshKey,
}: {
  account: string;
  refreshKey: string;
}) {
  const { queue, pending, error, confirm, dismiss } = useArtistReviewQueue(
    account,
    refreshKey,
  );
  const titleId = useId();
  if (!queue || queue.total === 0) return null;
  return (
    <section className="review-queue" aria-labelledby={titleId}>
      <details>
        <summary>
          <h2 className="discography-heading" id={titleId}>
            Needs your confirmation ({queue.total})
          </h2>
        </summary>
        <p className="muted">
          Hito could not tell these artists apart on its own. Confirming applies
          the artist to the undecided tracks of that credit.
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
                      <span>{details(candidate) || candidate.name}</span>
                      <Button
                        disabled={pending !== null}
                        aria-label={`Confirm ${label(candidate)}, for ${item.name}`}
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
      </details>
    </section>
  );
}
