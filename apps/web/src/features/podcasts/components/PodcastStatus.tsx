import type { PodcastSubscription } from "../../../shared/types";

export default function PodcastStatus({
  subscription,
}: {
  subscription: PodcastSubscription;
}) {
  const checked =
    subscription.last_refreshed_at == null
      ? null
      : new Date(subscription.last_refreshed_at * 1000);
  return (
    <div>
      {subscription.refresh_pending && <p role="status">Checking feed…</p>}
      {subscription.refresh_failed && (
        <p>
          The last feed check failed. Previously saved episodes remain
          available.
        </p>
      )}
      {checked ? (
        <p className="muted">
          Last checked{" "}
          <time dateTime={checked.toISOString()}>
            {checked.toLocaleString()}
          </time>
        </p>
      ) : (
        !subscription.refresh_pending && (
          <p className="muted">
            This feed has not been checked successfully yet.
          </p>
        )
      )}
    </div>
  );
}
