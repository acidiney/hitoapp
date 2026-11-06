import type { Schema } from "../../../shared/schema";
import type { RefObject } from "react";

import Artwork from "../../artwork/components/Artwork";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import { Check } from "lucide-react";
import {
  candidateArtist,
  candidateLength,
  lengthComparison,
} from "../model/candidateFacts";

function Side({
  label,
  heading = label,
  id,
  title,
  artist,
  album,
  durationMs,
  recording,
}: {
  label: string;
  heading?: string;
  id: string;
  title: string;
  artist: string;
  album: string | null | undefined;
  durationMs: number | null | undefined;
  recording?: string;
}) {
  return (
    <div className="review-side" role="group" aria-label={label}>
      <h4>{heading}</h4>
      <Artwork
        className="review-cover"
        recording={recording}
        fallback={{ id, title, inline: false }}
      />
      <p className="review-title">{title}</p>
      <p>{candidateArtist(artist) ?? "Artist not listed"}</p>
      <p>{album || "—"}</p>
      <p className="track-mono">
        {candidateLength(durationMs) ?? "Length not listed"}
      </p>
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
          <span>Album</span>
          <span>Length</span>
        </div>
        <Side
          label="Saved track"
          id={detail.recording_id}
          title={detail.title.value}
          artist={detail.artist.value}
          album={detail.selected_observation?.album}
          durationMs={savedMs}
          recording={detail.recording_id}
        />
        <Side
          label="Candidate source"
          heading={provider ? `Candidate · ${provider}` : "Candidate source"}
          id={review.candidate.id}
          title={review.candidate.title}
          artist={review.candidate.artist}
          album={review.candidate.release?.title}
          durationMs={review.candidate.duration_ms}
        />
      </div>
      {comparison && (
        <p className="review-hint" data-close={comparison.close} role="status">
          {comparison.close && <Check size={16} aria-hidden="true" />}
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
