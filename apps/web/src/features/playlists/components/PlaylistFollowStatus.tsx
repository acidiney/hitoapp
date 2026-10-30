import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";

import Button from "../../../shared/ui/Button";

type FollowStatusProps = {
  follow: Schema<"PlaylistFollow"> | null;
  loading: boolean;
  pending: boolean;
  hasAttempt: boolean;
  statusError: string;
  refreshError: string;
  detailError: string;
  refresh: () => Promise<boolean>;
  retryRefresh: () => Promise<boolean>;
  retryStatus: () => Promise<void>;
};

const labels: Record<Schema<"PlaylistFollow">["status"], string> = {
  ready: "Ready",
  queued: "Queued",
  running: "Updating",
  failed: "Update failed",
  paused: "Paused",
};

export default function PlaylistFollowStatus({
  follow,
  loading,
  pending,
  hasAttempt,
  statusError,
  refreshError,
  detailError,
  refresh,
  retryRefresh,
  retryStatus,
}: FollowStatusProps) {
  const paused = follow?.status === "paused";
  const times = follow
    ? [
        { label: "Last checked", seconds: follow.last_refreshed_at },
        { label: "Next check", seconds: follow.next_refresh_at },
      ]
    : [];
  return (
    <section aria-label="Playlist updates">
      <h2>Playlist updates</h2>
      {follow ? (
        <>
          <p role="status">{labels[follow.status]}</p>
          <p>{follow.interval === "hourly" ? "Every hour" : "Every day"}</p>
          <dl>
            {times.map(({ label, seconds }) => {
              const date = new Date(seconds * 1000);
              return (
                <div key={label}>
                  <dt>{label}</dt>
                  <dd>
                    <time dateTime={date.toISOString()}>
                      {date.toLocaleString()}
                    </time>
                  </dd>
                </div>
              );
            })}
          </dl>
          {follow.status === "failed" && (
            <p>The last update failed. Your saved entries remain available.</p>
          )}
          {paused && (
            <p>
              Updates are paused because this playlist's connection changed.
              Your saved entries remain available.{" "}
              <Link href="/settings?section=plugins">
                Check plugin settings
              </Link>
            </p>
          )}
        </>
      ) : (
        loading && <p role="status">Checking playlist updates…</p>
      )}
      <Button
        disabled={!follow || loading || pending || paused || hasAttempt}
        onClick={() => void refresh()}
      >
        Refresh now
      </Button>
      {statusError && (
        <>
          <p role="alert">{statusError}</p>
          <Button onClick={() => void retryStatus()}>Retry status check</Button>
        </>
      )}
      {refreshError && <p role="alert">{refreshError}</p>}
      {detailError && (
        <>
          <p role="alert">{detailError}</p>
          <Button onClick={() => void retryStatus()}>
            Retry playlist details
          </Button>
        </>
      )}
      {hasAttempt && (
        <Button
          disabled={pending || paused}
          onClick={() => void retryRefresh()}
        >
          Retry refresh request
        </Button>
      )}
    </section>
  );
}
