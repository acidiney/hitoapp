import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";
import { candidateLength } from "../../sources/model/candidateFacts";
import { sourceName, type TrackStatus } from "../model/trackStatus";
import type { TrackAnchor, TrackTab } from "../model/trackTabs";
import {
  audioDetail,
  lastPlayedText,
  trackPositionText,
} from "../model/trackFacts";
import CreditLine from "./CreditLine";

const PROOF: Record<NonNullable<Schema<"RecordingCopy">["proof"]>, string> = {
  musicbrainz_recording: "same recording",
  isrc: "same ISRC",
};

function Fact({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="track-fact">
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  );
}

function LyricsPreview({ lines, onOpen }: { lines: string[]; onOpen(): void }) {
  return (
    <div className="track-block">
      <div className="track-block-head">
        <h2>Lyrics</h2>
        <button type="button" className="text-link" onClick={onOpen}>
          {lines.length > 0 ? "All lyrics" : "Look up lyrics"}
        </button>
      </div>
      {lines.length > 0 ? (
        <ol className="track-lyrics-preview" aria-label="Lyrics preview">
          {lines.slice(0, 4).map((line, index) => (
            <li key={index}>{line}</li>
          ))}
        </ol>
      ) : (
        <p className="track-muted">No lyrics yet.</p>
      )}
    </div>
  );
}

export default function TrackOverview({
  detail,
  copies,
  status,
  localFile,
  choice,
  lyricPreview,
  onSelect,
  onReveal,
}: {
  detail: Schema<"RecordingMetadata">;
  copies: Schema<"RecordingCopies">;
  status: TrackStatus;
  localFile: boolean;
  choice: Schema<"SourceChoice"> | null;
  lyricPreview: string[];
  onSelect(tab: TrackTab): void;
  onReveal(anchor: TrackAnchor): void;
}) {
  const observation = detail.selected_observation;
  const length = candidateLength(
    observation && observation.duration_ms > 0 ? observation.duration_ms : null,
  );
  const active =
    choice?.sources.find((entry) => entry.selected && entry.source.ready) ??
    choice?.sources.find((entry) => entry.source.ready);
  const others = copies.copies.filter(
    (copy) => copy.recording_id !== detail.recording_id,
  );
  const appearances = detail.appears_in;
  const isrc = observation?.tags?.isrc?.value ?? null;
  const identified = detail.musicbrainz_recording_id !== null || isrc !== null;
  const hasRail = others.length > 0 || identified || appearances.length > 0;
  const position = trackPositionText(
    observation?.track_number ?? null,
    observation?.disc_number ?? null,
    detail.release_facts.track_total,
  );
  const albumAside = [position, detail.release_facts.label]
    .filter(Boolean)
    .join(" · ");
  const format = audioDetail(detail.audio_format);
  const lastPlayed = lastPlayedText(detail.last_played);
  return (
    <div className="track-overview" data-rail={hasRail}>
      <div className="track-overview-main">
        <dl className="track-facts">
          <Fact label="Title">
            {detail.title.value}
            {detail.title.personal !== null && (
              <span className="track-chip" data-tone="action">
                Personal
              </span>
            )}
          </Fact>
          <Fact label="Artist">
            <CreditLine
              credits={detail.credited_artists}
              fallback={detail.artist.value}
            />
            {detail.artist.personal !== null && (
              <span className="track-chip" data-tone="action">
                Personal
              </span>
            )}
          </Fact>
          {observation?.album && (
            <Fact label="Album">
              {observation.release_id ? (
                <Link
                  className="track-link"
                  href={`/albums/${encodeURIComponent(observation.release_id)}`}
                >
                  {observation.album}
                </Link>
              ) : (
                observation.album
              )}
              {albumAside && (
                <span className="track-aside">{`· ${albumAside}`}</span>
              )}
            </Fact>
          )}
          {length && (
            <Fact label="Length">
              <span className="track-mono">{length}</span>
            </Fact>
          )}
          {format && (
            <Fact label="Format">
              <span className="track-mono">{format}</span>
            </Fact>
          )}
          <Fact label="Source">
            {active || localFile ? (
              <>
                {active ? sourceName(active.source) : "Local file"}
                <span className="track-chip" data-tone="success">
                  Ready
                </span>
                {choice && (
                  <span className="track-aside">
                    {status.pinned ? "· Pinned" : "· Automatic"}
                  </span>
                )}
              </>
            ) : (
              <>
                <span className="track-chip" data-tone="warning">
                  None
                </span>
                <button
                  type="button"
                  className="text-link"
                  onClick={() => onReveal("find-audio")}
                >
                  Find audio
                </button>
              </>
            )}
          </Fact>
          {lastPlayed && <Fact label="Last played">{lastPlayed}</Fact>}
        </dl>
        <LyricsPreview lines={lyricPreview} onOpen={() => onSelect("lyrics")} />
      </div>
      {hasRail && (
        <aside className="track-rail" aria-label="Related">
          {appearances.length > 0 && (
            <section>
              <h2>Appears in</h2>
              <ul>
                {appearances.map((playlist) => (
                  <li key={playlist.id}>
                    <Link
                      className="track-link"
                      href={`/playlists/${encodeURIComponent(playlist.id)}`}
                    >
                      {playlist.name}
                    </Link>
                  </li>
                ))}
              </ul>
            </section>
          )}
          {others.length > 0 && (
            <section>
              <h2>Copies</h2>
              <ul>
                {others.map((copy) => (
                  <li key={copy.recording_id}>
                    <Link
                      className="track-link"
                      href={`/recordings/${encodeURIComponent(copy.recording_id)}`}
                    >
                      {copy.album ?? copy.title}
                    </Link>
                    {copy.proof && <span>{PROOF[copy.proof]}</span>}
                  </li>
                ))}
              </ul>
              <button
                type="button"
                className="text-link"
                onClick={() => onReveal("copies")}
              >
                Manage copies
              </button>
            </section>
          )}
          {identified && (
            <section>
              <h2>Identifiers</h2>
              <dl>
                {detail.musicbrainz_recording_id !== null && (
                  <div>
                    <dt>MusicBrainz</dt>
                    <dd>
                      <a
                        href={`https://musicbrainz.org/recording/${encodeURIComponent(detail.musicbrainz_recording_id)}`}
                        target="_blank"
                        rel="noreferrer noopener"
                      >
                        Open
                      </a>
                    </dd>
                  </div>
                )}
                {isrc !== null && (
                  <div>
                    <dt>ISRC</dt>
                    <dd>
                      <span className="track-mono track-id">{isrc}</span>
                    </dd>
                  </div>
                )}
              </dl>
            </section>
          )}
        </aside>
      )}
    </div>
  );
}
