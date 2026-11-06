import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";

import { useRef } from "react";
import { useSourceAttachment } from "../hooks/useSourceAttachment";
import DiscoverySearch from "../../discovery/components/DiscoverySearch";
import Button from "../../../shared/ui/Button";
import Artwork from "../../artwork/components/Artwork";
import SourceCandidateReview from "./SourceCandidateReview";
import { candidateArtist, candidateLength } from "../model/candidateFacts";

export default function SourceAttachment({
  account,
  detail,
  prominent,
  onAttached,
}: {
  account: string;
  detail: Schema<"RecordingMetadata">;
  prominent: boolean;
  onAttached(): void;
}) {
  const workflow = useSourceAttachment(
    account,
    detail.recording_id,
    onAttached,
  );
  const opener = useRef<HTMLElement | null>(null);
  const provider = workflow.connection
    ? (workflow.names[workflow.connection.service_id] ??
      workflow.connection.manifest.name)
    : "";
  return (
    <section
      className="rd-card source-attachment"
      aria-label="Add an audio source"
    >
      <h2>Add an audio source</h2>
      <p>
        Find audio through your connected music services and review the match
        before adding it to this saved track.
      </p>
      {!workflow.loaded && (
        <Button
          variant={prominent ? "primary" : "secondary"}
          disabled={workflow.pending !== null}
          onClick={() => void workflow.open()}
        >
          {workflow.pending === "settings"
            ? "Loading services…"
            : "Find a source"}
        </Button>
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
            <ul className="rd-rows" aria-label="Source search results">
              {workflow.items.map((hit) => (
                <li className="rd-row source-candidate" key={hit.id}>
                  <Artwork
                    className="source-candidate-cover"
                    fallback={{
                      id: hit.release?.id ?? hit.id,
                      title: hit.release?.title ?? hit.title,
                      inline: false,
                    }}
                  />
                  <div className="source-candidate-copy">
                    <h3>{hit.title}</h3>
                    {candidateArtist(hit.artist) && (
                      <p>{candidateArtist(hit.artist)}</p>
                    )}
                    <p className="source-candidate-facts">
                      {[candidateLength(hit.duration_ms), provider]
                        .filter(Boolean)
                        .join(" · ")}
                    </p>
                  </div>
                  <Button
                    className="rd-row-action"
                    disabled={workflow.pending !== null}
                    onClick={(event) => {
                      opener.current = event.currentTarget;
                      void workflow.reviewCandidate(hit);
                    }}
                  >
                    Review match
                  </Button>
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
        </fieldset>
      )}
      {workflow.review && (
        <SourceCandidateReview
          detail={detail}
          review={workflow.review.detail}
          provider={provider}
          pending={workflow.pending === "attach"}
          opener={opener}
          onConfirm={() => void workflow.confirm()}
          onClose={workflow.dismissReview}
        />
      )}
      {workflow.attached && <p role="status">Source added to this track.</p>}
    </section>
  );
}
