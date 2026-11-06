import type { Schema } from "../../../shared/schema";
import type { RefObject } from "react";

import Artwork from "../../artwork/components/Artwork";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import {
  candidateArtist,
  candidateLength,
  lengthComparison,
} from "../model/candidateFacts";

function Side({
  label,
  id,
  title,
  artist,
  durationMs,
  recording,
}: {
  label: string;
  id: string;
  title: string;
  artist: string;
  durationMs: number | null | undefined;
  recording?: string;
}) {
  return (
    <div className="review-side" role="group" aria-label={label}>
      <h4>{label}</h4>
      <Artwork
        className="review-cover"
        recording={recording}
        fallback={{ id, title, inline: false }}
      />
      <p className="review-title">{title}</p>
      <p>{candidateArtist(artist) ?? "Artist not listed"}</p>
      <p>{candidateLength(durationMs) ?? "Length not listed"}</p>
    </div>
  );
}

export default function SourceCandidateReview({
  detail,
  review,
  provider,
  pending,
  opener,
  onConfirm,
  onClose,
}: {
  detail: Schema<"RecordingMetadata">;
  review: Schema<"ProviderSourceReview">;
  provider: string;
  pending: boolean;
  opener: RefObject<HTMLElement | null>;
  onConfirm(): void;
  onClose(): void;
}) {
  const savedMs = detail.selected_observation?.duration_ms;
  const comparison = lengthComparison(savedMs, review.candidate.duration_ms);
  return (
    <Dialog
      wide
      title="Review source match"
      description={`Confirm this ${provider} result is the same track. Your saved details and playlists are kept.`}
      busy={pending}
      returnFocus={opener}
      onClose={onClose}
    >
      <div className="review-compare" data-aligned="true">
        <div className="review-labels" aria-hidden="true">
          <span />
          <span />
          <span>Title</span>
          <span>Artist</span>
          <span>Length</span>
        </div>
        <Side
          label="Saved track"
          id={detail.recording_id}
          title={detail.title.value}
          artist={detail.artist.value}
          durationMs={savedMs}
          recording={detail.recording_id}
        />
        <Side
          label="Candidate source"
          id={review.candidate.id}
          title={review.candidate.title}
          artist={review.candidate.artist}
          durationMs={review.candidate.duration_ms}
        />
      </div>
      {comparison && (
        <p className="review-hint" data-close={comparison.close} role="status">
          {comparison.text}
        </p>
      )}
      <footer className="review-actions">
        <Button disabled={pending} onClick={onClose}>
          Cancel
        </Button>
        <Button variant="primary" disabled={pending} onClick={onConfirm}>
          {pending ? "Adding source…" : "Use this source"}
        </Button>
      </footer>
    </Dialog>
  );
}
