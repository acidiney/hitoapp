import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";
import { PluginErrorText } from "../../plugins/components/PluginSettingsLink";

import { useEffect, useRef } from "react";
import { useSourceAttachment } from "../hooks/useSourceAttachment";
import DiscoverySearch from "../../discovery/components/DiscoverySearch";
import Button from "../../../shared/ui/Button";
import Artwork from "../../artwork/components/Artwork";
import SourceCandidateReview from "./SourceCandidateReview";
import { Check } from "lucide-react";
import {
  candidateArtist,
  candidateLength,
  lengthComparison,
} from "../model/candidateFacts";

export default function SourceAttachment({
  account,
  detail,
  prominent,
  expanded = false,
  defaultQuery = "",
  onAttached,
}: {
  account: string;
  detail: Schema<"RecordingMetadata">;
  prominent: boolean;
  expanded?: boolean;
  defaultQuery?: string;
  onAttached(): void;
}) {
  const workflow = useSourceAttachment(
    account,
    detail.recording_id,
    onAttached,
  );
  const opener = useRef<HTMLElement | null>(null);
  const opened = useRef(false);
  const open = workflow.open;
  useEffect(() => {
    if (!expanded || opened.current) return;
    opened.current = true;
    void open();
  }, [expanded, open]);
  const provider = workflow.connection
    ? (workflow.names[workflow.connection.service_id] ??
      workflow.connection.manifest.name)
    : "";
  const savedMs = detail.selected_observation?.duration_ms;
  const sameTrack = (hit: Schema<"ProviderRecording">) =>
    lengthComparison(savedMs, hit.duration_ms)?.close === true &&
    candidateArtist(hit.artist)?.toLocaleLowerCase() ===
      detail.artist.value.trim().toLocaleLowerCase();
  return (
    <section
      className="rd-card source-attachment"
      aria-labelledby="find-audio"
      data-expanded={expanded || undefined}
    >
      <h2 id="find-audio" tabIndex={-1}>
        {prominent ? "Find audio" : "Add another source"}
      </h2>
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
      {workflow.error && (
        <p role="alert">
          <PluginErrorText message={workflow.error} />
          {workflow.existing && workflow.existing !== detail.recording_id && (
            <>
              {" "}
              <Link
                href={`/recordings/${encodeURIComponent(workflow.existing)}`}
              >
                Open that track
              </Link>
            </>
          )}
        </p>
      )}
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
            pills={expanded}
            initialQuery={defaultQuery}
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
                    {(candidateArtist(hit.artist) || hit.release?.title) && (
                      <p>
                        {[candidateArtist(hit.artist), hit.release?.title]
                          .filter(Boolean)
                          .join(" · ")}
                      </p>
                    )}
                    <p className="source-candidate-facts">
                      {[candidateLength(hit.duration_ms), provider]
                        .filter(Boolean)
                        .join(" · ")}
                    </p>
                  </div>
                  <div className="source-candidate-end">
                    {sameTrack(hit) && (
                      <span className="track-ok">
                        <Check size={14} aria-hidden="true" /> Same length and
                        artist
                      </span>
                    )}
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
                  </div>
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
