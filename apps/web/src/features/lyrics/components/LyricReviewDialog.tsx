import type { RefObject } from "react";
import type { Schema } from "../../../shared/schema";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import {
  candidateArtist,
  candidateLength,
} from "../../sources/model/candidateFacts";
import { lyricIdentityLimit, type LyricService } from "../model/lyricIdentity";
import type { LyricFailure } from "../model/lyricFailure";
import type { LyricCandidate } from "../hooks/useLyricSearch";
import PluginSettingsLink from "../../plugins/components/PluginSettingsLink";
import LyricState from "./LyricState";

type Facts = {
  title: string;
  artist: string | null | undefined;
  album: string | null | undefined;
  durationMs: number | null | undefined;
};

export type LyricReviewProps = {
  saved: Facts;
  service: LyricService;
  candidate: LyricCandidate;
  phase: "idle" | "reviewing" | "reviewed" | "accepting";
  review: Schema<"RecordingLyricReview"> | null;
  failure: LyricFailure | null;
  duplicate: boolean;
  atLimit: boolean;
  opener: RefObject<HTMLElement | null>;
  onAccept: () => void;
  onReviewAgain: () => void;
  onClose: () => void;
};

function Side({ label, facts }: { label: string; facts: Facts }) {
  return (
    <div className="review-side" role="group" aria-label={label}>
      <h4>{label}</h4>
      <p className="review-title">{facts.title}</p>
      <p>{candidateArtist(facts.artist ?? "") ?? "Artist not listed"}</p>
      <p>{facts.album || "Album not listed"}</p>
      <p className="track-mono">
        {candidateLength(facts.durationMs) ?? "Length not listed"}
      </p>
    </div>
  );
}

export default function LyricReviewDialog({
  saved,
  service,
  candidate,
  phase,
  review,
  failure,
  duplicate,
  atLimit,
  opener,
  onAccept,
  onReviewAgain,
  onClose,
}: LyricReviewProps) {
  const reviewed = review?.candidate;
  const facts: Facts = reviewed
    ? {
        title: reviewed.recording.title,
        artist: reviewed.recording.artist,
        album: reviewed.album_title ?? reviewed.recording.release?.title,
        durationMs: reviewed.recording.duration_ms,
      }
    : {
        title: candidate.title,
        artist: candidate.artist,
        album: candidate.release?.title,
        durationMs: candidate.duration_ms,
      };
  const blocked = atLimit && !duplicate;
  return (
    <Dialog
      wide
      title="Are these the lyrics you want?"
      description={`Check that this ${service.label} match is the same track. Nothing is saved until you choose.`}
      busy={phase === "accepting"}
      returnFocus={opener}
      onClose={onClose}
    >
      <div className="review-compare">
        <Side label="Saved track" facts={saved} />
        <Side label={`Match from ${service.label}`} facts={facts} />
      </div>
      {phase === "reviewing" && (
        <p role="status">
          <LyricState tone="checking">Loading the match details…</LyricState>
        </p>
      )}
      {duplicate && phase === "reviewed" && (
        <p className="review-hint" data-close="true" role="status">
          This lyric choice is already saved for this track.
        </p>
      )}
      {blocked && phase === "reviewed" && (
        <p className="review-hint" role="note">
          This track already keeps {lyricIdentityLimit} lyric choices. Close
          this and remove one first.
        </p>
      )}
      {failure && (
        <p className="review-hint" role="alert">
          {failure.message}
          {failure.kind === "credentials" && (
            <>
              {" "}
              <PluginSettingsLink />
            </>
          )}
        </p>
      )}
      <footer className="review-actions">
        <Button disabled={phase === "accepting"} onClick={onClose}>
          Cancel
        </Button>
        {failure ? (
          <Button variant="primary" onClick={onReviewAgain}>
            Review again
          </Button>
        ) : (
          <Button
            variant="primary"
            disabled={phase !== "reviewed" || blocked}
            onClick={onAccept}
          >
            {phase === "accepting" ? "Saving…" : "Use these lyrics"}
          </Button>
        )}
      </footer>
    </Dialog>
  );
}
