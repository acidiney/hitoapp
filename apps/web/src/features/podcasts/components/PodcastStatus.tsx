import type { Schema } from "../../../shared/schema";

export function checkedAt(
  subscription: Schema<"PodcastSubscription">,
): Date | null {
  return subscription.last_refreshed_at == null
    ? null
    : new Date(subscription.last_refreshed_at * 1000);
}

export default function PodcastStatus({
  subscription,
}: {
  subscription: Schema<"PodcastSubscription">;
}) {
  const checked = checkedAt(subscription);
  return (
    <div className="podcast-status">
      {subscription.refresh_pending && <p role="status">Checking feed…</p>}
      {subscription.refresh_failed && (
        <p>
          The last feed check failed. Previously saved episodes remain
          available.
        </p>
      )}
      {checked ? (
        <p className="media-mono">
          Last checked{" "}
          <time dateTime={checked.toISOString()}>
            {checked.toLocaleString()}
          </time>
        </p>
      ) : (
        !subscription.refresh_pending && (
          <p className="media-mono">
            This feed has not been checked successfully yet.
          </p>
        )
      )}
    </div>
  );
}
