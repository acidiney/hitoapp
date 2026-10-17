import { Link } from "@inertiajs/react";
import EmptyState from "../components/EmptyState";
import PageHeading from "../components/PageHeading";
import PodcastEpisodes from "../components/podcasts/PodcastEpisodes";
import PodcastStatus from "../components/podcasts/PodcastStatus";
import { useMutation } from "../hooks/useMutation";
import { usePodcastEpisodes } from "../hooks/usePodcastEpisodes";
import type { PodcastPageProps } from "../types";

export default function PodcastPage({
  user,
  playlists,
  subscription,
  episodes,
  progress,
}: PodcastPageProps) {
  const { mutate } = useMutation(user.id);
  const workflow = usePodcastEpisodes(
    user.id,
    subscription.subscription_id,
    episodes,
    progress,
  );
  return (
    <>
      <PageHeading
        title={subscription.title || "Untitled podcast"}
        description={`${workflow.page.total} ${workflow.page.total === 1 ? "episode" : "episodes"}`}
      >
        <Link className="button" href="/podcasts">
          All podcasts
        </Link>
      </PageHeading>
      <PodcastStatus subscription={subscription} />
      {workflow.loading && (
        <p role="status">Loading episodes and saved positions…</p>
      )}
      {workflow.error && (
        <div>
          <p role="alert">{workflow.error}</p>
          <button
            className="button"
            disabled={workflow.loading}
            onClick={() =>
              void (workflow.stale ? workflow.first() : workflow.retry())
            }
          >
            {workflow.stale ? "Load latest episodes" : "Retry episodes"}
          </button>
        </div>
      )}
      {workflow.page.items.length ? (
        <PodcastEpisodes
          episodes={workflow.page.items}
          progress={workflow.progress}
          playlists={playlists}
          mutate={mutate}
        />
      ) : (
        <EmptyState
          title="No episodes yet"
          text="Episodes appear here after a successful feed check."
          libraryLink={false}
        />
      )}
      {(workflow.offset > 0 || workflow.page.next_cursor) && (
        <nav className="pagination" aria-label="Episode pages">
          <button
            className="button"
            disabled={workflow.loading || workflow.offset === 0}
            onClick={() => void workflow.first()}
          >
            First episodes
          </button>
          <span>
            {workflow.offset + 1} to{" "}
            {workflow.offset + workflow.page.items.length} of {workflow.page.total}
          </span>
          <button
            className="button"
            disabled={
              workflow.loading || workflow.stale || !workflow.page.next_cursor
            }
            onClick={() => void workflow.next()}
          >
            Next episodes
          </button>
        </nav>
      )}
    </>
  );
}
