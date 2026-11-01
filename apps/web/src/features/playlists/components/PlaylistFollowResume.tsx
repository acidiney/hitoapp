import type { Schema } from "../../../shared/schema";
import { ArrowRight } from "lucide-react";
import { Link } from "@inertiajs/react";
import Button from "../../../shared/ui/Button";
import {
  changeLabel,
  changeValue,
  reviewSentence,
} from "../model/followReview";

type FollowResumeProps = {
  follow: Schema<"PlaylistFollow"> | null;
  pending: boolean;
  hasAttempt: boolean;
  error: string;
  resume: () => Promise<boolean>;
  retry: () => Promise<boolean>;
};

export default function PlaylistFollowResume({
  follow,
  pending,
  hasAttempt,
  error,
  resume,
  retry,
}: FollowResumeProps) {
  const paused = follow?.status === "paused";
  const review = paused ? (follow.review ?? null) : null;
  if (!paused && !hasAttempt && !error) return null;
  return (
    <section
      className="status-strip follow-resume"
      aria-label="Resume playlist updates"
    >
      <h2 className="sr-only">Resume playlist updates</h2>
      <div className="status-strip-main">
        {review && (
          <>
            <p className="status-strip-line">
              <strong>{reviewSentence(review)}</strong>
            </p>
            <dl className="follow-changes">
              {review.changes.map((change) => (
                <div key={change.kind}>
                  <dt>{changeLabel(change)}</dt>
                  <dd>
                    <span className="follow-change-before">
                      {changeValue(change.before)}
                    </span>
                    <ArrowRight size={14} aria-hidden="true" />
                    <span>{changeValue(change.after)}</span>
                  </dd>
                </div>
              ))}
            </dl>
          </>
        )}
        {paused && (
          <p className="status-strip-note">
            Your saved entries remain available.
          </p>
        )}
        {error && <p role="alert">{error}</p>}
      </div>
      <div className="status-strip-actions">
        {review?.resumable && !hasAttempt && (
          <Button
            variant="primary"
            disabled={pending}
            onClick={() => void resume()}
          >
            Resume updates
          </Button>
        )}
        {hasAttempt && (
          <Button disabled={pending} onClick={() => void retry()}>
            Retry resume request
          </Button>
        )}
        {paused && (
          <Link className="button quiet" href="/settings?section=plugins">
            Review connection
          </Link>
        )}
      </div>
    </section>
  );
}
