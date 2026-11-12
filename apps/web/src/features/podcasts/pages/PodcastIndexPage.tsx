import LibraryHeading from "../../../app/LibraryHeading";
import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";
import { friendlyTime } from "../../../shared/format";
import Button from "../../../shared/ui/Button";
import EmptyState from "../../../shared/ui/EmptyState";
import FallbackCover from "../../../shared/ui/FallbackCover";
import PodcastSubscribe from "../components/PodcastSubscribe";
import { checkedAt } from "../components/PodcastStatus";
import { usePodcastSubscriptions } from "../hooks/usePodcastSubscriptions";

function status(subscription: Schema<"PodcastSubscription">) {
  if (subscription.refresh_pending) return "Checking feed…";
  const checked = checkedAt(subscription);
  if (!checked) return "Not checked yet";
  return (
    <>
      Last checked{" "}
      <time dateTime={checked.toISOString()}>{friendlyTime(checked)}</time>
    </>
  );
}

export default function PodcastIndexPage({
  user,
  subscriptions,
}: Schema<"PodcastIndexPageProps">) {
  const workflow = usePodcastSubscriptions(user.id, subscriptions);
  const count = workflow.subscriptions.length;
  return (
    <>
      <LibraryHeading
        path="/podcasts"
        title="Podcasts"
        description={`${count} ${count === 1 ? "subscription" : "subscriptions"}`}
      >
        <div className="library-heading-actions">
          <PodcastSubscribe
            key={user.id}
            pending={workflow.pending}
            hasAttempt={workflow.hasAttempt}
            error={workflow.error}
            subscribe={workflow.subscribe}
            retry={workflow.retry}
          />
        </div>
      </LibraryHeading>
      {workflow.statusError && (
        <div>
          <p role="alert">{workflow.statusError}</p>
          <Button
            disabled={workflow.loading || workflow.pending}
            onClick={() => void workflow.check()}
          >
            Check again
          </Button>
        </div>
      )}
      {count > 0 && (
        <ul className="media-grid" aria-label="Subscribed podcasts">
          {workflow.subscriptions.map((subscription) => {
            const title = subscription.title || "Untitled podcast";
            return (
              <li className="media-card" key={subscription.subscription_id}>
                <div aria-hidden="true" className="media-card-cover">
                  <FallbackCover
                    id={subscription.subscription_id}
                    title={title}
                    artist="Podcast"
                    kind="podcast"
                    inline={false}
                  />
                </div>
                <Link
                  className="media-card-title"
                  href={`/podcasts/${encodeURIComponent(subscription.subscription_id)}`}
                >
                  {title}
                </Link>
                <span className="media-mono">{status(subscription)}</span>
                {subscription.refresh_failed && (
                  <span className="media-warn">Last feed check failed</span>
                )}
              </li>
            );
          })}
        </ul>
      )}
      {count === 0 && (
        <EmptyState
          title="No podcast subscriptions yet"
          text="Subscribe with a feed address to follow a show. New episodes appear on its page after a feed check."
          libraryLink={false}
        />
      )}
    </>
  );
}
