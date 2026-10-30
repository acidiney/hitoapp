import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";

import Button from "../../../shared/ui/Button";
import HealthDot from "../../../shared/ui/HealthDot";
import { friendlyTime } from "../../../shared/format";

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
  attribution: string;
  notice: string;
};

const labels: Record<Schema<"PlaylistFollow">["status"], string> = {
  ready: "Ready",
  queued: "Queued",
  running: "Updating",
  failed: "Update failed",
  paused: "Paused",
};

const tones: Record<
  Schema<"PlaylistFollow">["status"],
  "success" | "warning" | "action"
> = {
  ready: "success",
  queued: "action",
  running: "action",
  failed: "warning",
  paused: "warning",
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
  attribution,
  notice,
}: FollowStatusProps) {
  const paused = follow?.status === "paused";
  const now = new Date();
  const times = follow
    ? [
        { label: "Last checked", seconds: follow.last_refreshed_at },
        { label: "Next check", seconds: follow.next_refresh_at },
      ]
    : [];
  return (
    <section
      className="status-strip follow-strip"
      aria-label="Playlist updates"
    >
      <h2 className="sr-only">Playlist updates</h2>
      <div className="status-strip-main">
        <p className="status-strip-line">
          <strong>Followed playlist.</strong>
          {follow && (
            <>
              <span role="status" className="follow-state">
                <HealthDot
                  tone={tones[follow.status]}
                  label={labels[follow.status]}
                />
              </span>
              <span className="status-strip-muted">
                {follow.interval === "hourly" ? "Every hour" : "Every day"}
              </span>
            </>
          )}
        </p>
        {follow ? (
          <>
            <dl className="follow-times">
              {times.map(({ label, seconds }) => {
                const date = new Date(seconds * 1000);
                return (
                  <div key={label}>
                    <dt>{label}</dt>
                    <dd>
                      <time dateTime={date.toISOString()}>
                        {friendlyTime(date, now)}
                      </time>
                    </dd>
                  </div>
                );
              })}
            </dl>
            {follow.status === "failed" && (
              <p className="status-strip-note">
                The last update failed. Your saved entries remain available.
              </p>
            )}
            {paused && (
              <p className="status-strip-note">
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
        <p className="status-strip-note">
          <span>{attribution}</span>
          {" · "}
          <span>{notice}</span>
        </p>
        {statusError && <p role="alert">{statusError}</p>}
        {refreshError && <p role="alert">{refreshError}</p>}
        {detailError && <p role="alert">{detailError}</p>}
      </div>
      <div className="status-strip-actions">
        <Button
          disabled={!follow || loading || pending || paused || hasAttempt}
          onClick={() => void refresh()}
        >
          Refresh now
        </Button>
        {statusError && (
          <Button onClick={() => void retryStatus()}>Retry status check</Button>
        )}
        {detailError && (
          <Button onClick={() => void retryStatus()}>
            Retry playlist details
          </Button>
        )}
        {hasAttempt && (
          <Button
            disabled={pending || paused}
            onClick={() => void retryRefresh()}
          >
            Retry refresh request
          </Button>
        )}
      </div>
    </section>
  );
}
