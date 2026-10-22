import { Link } from "@inertiajs/react";
import type { FollowResumeReview } from "../hooks/useFollowResume";

type FollowResumeProps = {
  paused: boolean;
  review: FollowResumeReview | null;
  pending: boolean;
  hasAttempt: boolean;
  error: string;
  reviewConnection: () => Promise<boolean>;
  resume: () => Promise<boolean>;
  retry: () => Promise<boolean>;
};

export default function PlaylistFollowResume({
  paused,
  review,
  pending,
  hasAttempt,
  error,
  reviewConnection,
  resume,
  retry,
}: FollowResumeProps) {
  if (!paused && !hasAttempt && !error) return null;
  return (
    <section aria-label="Resume playlist updates">
      <h2>Resume playlist updates</h2>
      <p>
        Review the original connection before allowing it to update this
        playlist again. Your saved entries remain available.
      </p>
      <button
        className="button"
        disabled={pending || hasAttempt || !paused}
        onClick={() => void reviewConnection()}
      >
        Review connection
      </button>
      {review && (
        <>
          <dl>
            <dt>Connection</dt>
            <dd>{review.installation.manifest.name}</dd>
            <dt>Declared capabilities</dt>
            <dd>
              {review.installation.manifest.capabilities
                .map((value) => value.replaceAll("_", " "))
                .join(", ")}
            </dd>
            <dt>Service</dt>
            <dd>{review.service.name}</dd>
            <dt>Service address</dt>
            <dd>{review.service.base_url}</dd>
          </dl>
          {!hasAttempt && (
            <button
              className="button"
              disabled={pending || !paused}
              onClick={() => void resume()}
            >
              Resume updates
            </button>
          )}
        </>
      )}
      {error && (
        <>
          <p role="alert">{error}</p>
          <Link href="/settings?section=plugins">Check plugin settings</Link>
        </>
      )}
      {hasAttempt && (
        <button
          className="button"
          disabled={pending}
          onClick={() => void retry()}
        >
          Retry resume request
        </button>
      )}
    </section>
  );
}
