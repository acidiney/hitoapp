import { Link } from "@inertiajs/react";
import EmptyState from "../components/EmptyState";
import PageHeading from "../components/PageHeading";
import PodcastStatus from "../components/podcasts/PodcastStatus";
import PodcastSubscribe from "../components/podcasts/PodcastSubscribe";
import { usePodcastSubscriptions } from "../hooks/usePodcastSubscriptions";
import type { PodcastIndexPageProps } from "../types";

export default function PodcastIndexPage({
  user,
  subscriptions,
}: PodcastIndexPageProps) {
  const workflow = usePodcastSubscriptions(user.id, subscriptions);
  return (
    <>
      <PageHeading
        title="Podcasts"
        description="Your subscribed RSS feeds and episodes."
      />
      <PodcastSubscribe
        key={user.id}
        pending={workflow.pending}
        hasAttempt={workflow.hasAttempt}
        error={workflow.error}
        subscribe={workflow.subscribe}
        retry={workflow.retry}
      />
      {workflow.statusError && (
        <div>
          <p role="alert">{workflow.statusError}</p>
          <button
            className="button"
            disabled={workflow.loading || workflow.pending}
            onClick={() => void workflow.check()}
          >
            Check again
          </button>
        </div>
      )}
      {workflow.subscriptions.map((subscription) => (
        <section className="playlist-import" key={subscription.subscription_id}>
          <h2>
            <Link
              href={`/podcasts/${encodeURIComponent(subscription.subscription_id)}`}
            >
              {subscription.title || "Untitled podcast"}
            </Link>
          </h2>
          <PodcastStatus subscription={subscription} />
        </section>
      ))}
      {workflow.subscriptions.length === 0 && (
        <EmptyState
          title="No podcast subscriptions yet"
          text="Subscribe with a feed address to see its episodes here."
          libraryLink={false}
        />
      )}
    </>
  );
}
