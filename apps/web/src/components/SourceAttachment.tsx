import { Link } from "@inertiajs/react";
import type { RecordingMetadata } from "../shared/types";
import { time } from "../shared/format";
import { useSourceAttachment } from "../hooks/useSourceAttachment";
import DiscoverySearch from "../features/discovery/components/DiscoverySearch";

export default function SourceAttachment({
  account,
  detail,
  onAttached,
}: {
  account: string;
  detail: RecordingMetadata;
  onAttached(): void;
}) {
  const workflow = useSourceAttachment(
    account,
    detail.recording_id,
    onAttached,
  );
  return (
    <section className="source-attachment" aria-label="Add an audio source">
      <h2>Add an audio source</h2>
      <p>
        Find audio through your connected music services and review the match
        before adding it to this saved track.
      </p>
      {!workflow.loaded && (
        <button
          className="button"
          disabled={workflow.pending !== null}
          onClick={() => void workflow.open()}
        >
          {workflow.pending === "settings"
            ? "Loading services…"
            : "Find a source"}
        </button>
      )}
      {workflow.error && <p role="alert">{workflow.error}</p>}
      {workflow.loaded && !workflow.connection && (
        <p>
          No connected service offers both music search and audio sources.{" "}
          <Link href="/settings?section=plugins">Connect a music service</Link>
        </p>
      )}
      {workflow.connection && (
        <fieldset disabled={workflow.pending === "attach"}>
          <DiscoverySearch
            connections={workflow.connections}
            names={workflow.names}
            selected={workflow.connection.id}
            pending={workflow.pending === "search"}
            onChoose={workflow.choose}
            onSearch={workflow.search}
          />
          {workflow.items.length > 0 && (
            <ul aria-label="Source search results">
              {workflow.items.map((hit) => (
                <li key={hit.id}>
                  <h3>{hit.title}</h3>
                  <p>{hit.artist}</p>
                  <p>
                    {hit.duration_ms == null
                      ? "Duration unknown"
                      : time(hit.duration_ms / 1000)}
                  </p>
                  <button
                    className="button"
                    disabled={workflow.pending !== null}
                    onClick={() => void workflow.reviewCandidate(hit)}
                  >
                    Review match
                  </button>
                </li>
              ))}
            </ul>
          )}
          {workflow.query &&
            workflow.pending === null &&
            !workflow.error &&
            !workflow.items.length && (
              <p>No matching tracks. Try another track or artist.</p>
            )}
          {workflow.review && (
            <section aria-label="Review source match">
              <h3>Review source match</h3>
              <div>
                <h4>Saved track</h4>
                <p>{detail.title.value}</p>
                <p>{detail.artist.value}</p>
                <p>
                  {time((detail.selected_observation?.duration_ms ?? 0) / 1000)}
                </p>
              </div>
              <div>
                <h4>Candidate source</h4>
                <p>{workflow.review.detail.candidate.title}</p>
                <p>{workflow.review.detail.candidate.artist}</p>
                <p>
                  {workflow.review.detail.candidate.duration_ms == null
                    ? "Duration unknown"
                    : time(workflow.review.detail.candidate.duration_ms / 1000)}
                </p>
              </div>
              <p>
                Confirm this is the same track. Your saved details and playlists
                are kept.
              </p>
              <button
                className="button primary"
                disabled={workflow.pending !== null}
                onClick={() => void workflow.confirm()}
              >
                {workflow.pending === "attach"
                  ? "Adding source…"
                  : "Use this source"}
              </button>
            </section>
          )}
        </fieldset>
      )}
      {workflow.attached && <p role="status">Source added to this track.</p>}
    </section>
  );
}
