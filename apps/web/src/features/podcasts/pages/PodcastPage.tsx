import { useState } from "react";
import { Play } from "lucide-react";
import Artwork from "../../artwork/components/Artwork";
import Breadcrumbs from "../../../shared/ui/Breadcrumbs";
import Button from "../../../shared/ui/Button";
import CollectionHeader from "../../../shared/ui/CollectionHeader";
import EmptyState from "../../../shared/ui/EmptyState";
import SegmentedControl from "../../../shared/ui/SegmentedControl";
import { tintFor } from "../../../shared/ui/collectionTint";
import PodcastEpisodes from "../components/PodcastEpisodes";
import PodcastStatus from "../components/PodcastStatus";
import {
  PodcastNotices,
  PodcastOptions,
  PodcastRefreshButton,
} from "../components/PodcastControls";
import { useMutation } from "../../../shared/hooks/useMutation";
import { useEpisodeRows } from "../hooks/useEpisodeRows";
import { usePodcastEpisodes } from "../hooks/usePodcastEpisodes";
import { usePodcastLifecycle } from "../hooks/usePodcastLifecycle";
import type { PodcastPageProps } from "../../../shared/types";

type Filter = "unplayed" | "all";

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
  const lifecycle = usePodcastLifecycle(user.id, subscription, workflow);
  const [filter, setFilter] = useState<Filter>("unplayed");
  const { rows, play, resumable, latest } = useEpisodeRows(
    workflow.page.items,
    workflow.progress,
  );
  const visible =
    filter === "all" ? rows : rows.filter((row) => row.state.kind !== "played");
  const title = lifecycle.subscription.title || "Untitled podcast";
  const id = lifecycle.subscription.subscription_id;
  const total = workflow.page.total;
  const lead = resumable ?? latest;
  return (
    <>
      <CollectionHeader
        tint={tintFor(id)}
        size="podcast"
        crumbs={
          <Breadcrumbs
            crumbs={[
              { label: "Library", href: "/library" },
              { label: "Podcasts", href: "/podcasts" },
              { label: title },
            ]}
          />
        }
        cover={
          <Artwork
            recording={workflow.page.items[0]?.recording.id}
            release={workflow.page.items[0]?.recording.release_id ?? undefined}
            fallback={{ id, title, artist: "Podcast" }}
          />
        }
        eyebrow="Podcast"
        title={title}
        stats={`${total} ${total === 1 ? "episode" : "episodes"}`}
        extra={<PodcastStatus subscription={lifecycle.subscription} />}
        actions={
          <>
            {lead && (
              <button
                type="button"
                className="button tint-primary"
                onClick={() => play(lead.index, lead.track.playback_start)}
              >
                <Play size={17} fill="currentColor" />
                {resumable
                  ? `Continue · ${resumable.state.label}`
                  : "Play latest"}
              </button>
            )}
            <span className="tint-status">
              Following <span aria-hidden="true">✓</span>
            </span>
            <PodcastRefreshButton {...lifecycle} />
          </>
        }
      />
      <PodcastNotices {...lifecycle} />
      <div className="media-layout">
        <section className="media-main" aria-labelledby="episodes-heading">
          <div className="media-section-head">
            <h2 id="episodes-heading">Episodes</h2>
            <SegmentedControl<Filter>
              label="Episode filter"
              value={filter}
              onChange={setFilter}
              options={[
                { value: "unplayed", label: "Unplayed" },
                { value: "all", label: "All" },
              ]}
            />
          </div>
          {workflow.loading && (
            <p role="status">Loading episodes and saved positions…</p>
          )}
          {workflow.error && (
            <div>
              <p role="alert">{workflow.error}</p>
              <Button
                disabled={workflow.loading || workflow.refreshing}
                onClick={() =>
                  void (workflow.stale ? workflow.first() : workflow.retry())
                }
              >
                {workflow.stale ? "Load latest episodes" : "Retry episodes"}
              </Button>
            </div>
          )}
          {visible.length > 0 ? (
            <PodcastEpisodes
              rows={visible}
              playlists={playlists}
              mutate={mutate}
              play={play}
              onMutationAccepted={lifecycle.onMutationAccepted}
            />
          ) : rows.length > 0 ? (
            <div className="media-empty">
              <p>You have played every episode on this page.</p>
              <Button onClick={() => setFilter("all")}>
                Show all episodes
              </Button>
            </div>
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
                disabled={
                  workflow.loading ||
                  workflow.refreshing ||
                  workflow.offset === 0
                }
                onClick={() => void workflow.first()}
              >
                First episodes
              </button>
              <span>
                {workflow.offset + 1} to{" "}
                {workflow.offset + workflow.page.items.length} of{" "}
                {workflow.page.total}
              </span>
              <button
                className="button"
                disabled={
                  workflow.loading ||
                  workflow.refreshing ||
                  workflow.stale ||
                  !workflow.page.next_cursor
                }
                onClick={() => void workflow.next()}
              >
                Next episodes
              </button>
            </nav>
          )}
        </section>
        <PodcastOptions {...lifecycle} />
      </div>
    </>
  );
}
