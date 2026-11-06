import type { Schema } from "../../../shared/schema";
import { Play, Search } from "lucide-react";
import { Link } from "@inertiajs/react";
import Artwork from "../../artwork/components/Artwork";
import AvailabilityLabel from "../../../shared/ui/AvailabilityLabel";
import Breadcrumbs from "../../../shared/ui/Breadcrumbs";
import Button from "../../../shared/ui/Button";
import { candidateLength } from "../../sources/model/candidateFacts";
import type { TrackStatus } from "../model/trackStatus";
import CreditLine from "./CreditLine";
import TrackActions from "./TrackActions";

export default function TrackHeader({
  account,
  detail,
  liked,
  playlists,
  status,
  onPlay,
  onFindAudio,
}: {
  account: string;
  detail: Schema<"RecordingMetadata">;
  liked: boolean;
  playlists: Schema<"Playlist">[];
  status: TrackStatus;
  onPlay(): void;
  onFindAudio(): void;
}) {
  const source = detail.selected_observation;
  const album = source?.album ?? null;
  const length = candidateLength(
    source && source.duration_ms > 0 ? source.duration_ms : null,
  );
  const crumbs = [
    { label: "Library", href: "/library" },
    { label: "Albums", href: "/albums" },
    ...(album && source?.release_id
      ? [
          {
            label: album,
            href: `/albums/${encodeURIComponent(source.release_id)}`,
          },
        ]
      : []),
    { label: detail.title.value },
  ];
  return (
    <header className="track-header">
      <Breadcrumbs crumbs={crumbs} />
      <div className="track-hero">
        <Artwork
          className="track-cover"
          recording={detail.recording_id}
          fallback={{
            id: detail.recording_id,
            title: detail.title.value,
            artist: detail.artist.value,
            inline: false,
          }}
        />
        <div className="track-hero-copy">
          <span className="track-kicker">Track</span>
          <h1>{detail.title.value}</h1>
          <p className="track-credit">
            <strong>
              <CreditLine
                credits={detail.credited_artists}
                fallback={detail.artist.value}
              />
            </strong>
            {album && (
              <>
                {" · "}
                {source?.release_id ? (
                  <Link
                    className="track-credit-album"
                    href={`/albums/${encodeURIComponent(source.release_id)}`}
                  >
                    {album}
                  </Link>
                ) : (
                  album
                )}
              </>
            )}
          </p>
          <p className="track-status">
            <AvailabilityLabel state={status.state} label={status.label} />
            {status.pinned && (
              <span className="track-chip" data-tone="neutral">
                Pinned
              </span>
            )}
            {length && <span className="track-mono">{length}</span>}
          </p>
          <div className="track-actions-row">
            {status.playable === true || status.playable === null ? (
              <Button
                variant="primary"
                disabled={status.playable === null}
                onClick={onPlay}
              >
                <Play size={16} aria-hidden="true" /> Play
              </Button>
            ) : (
              <Button variant="action" onClick={onFindAudio}>
                <Search size={16} aria-hidden="true" /> Find audio
              </Button>
            )}
            <TrackActions
              account={account}
              liked={liked}
              playlists={playlists}
              track={{
                id: detail.recording_id,
                title: detail.title.value,
                artist: detail.artist.value,
                album: source?.album ?? null,
                duration_ms: source?.duration_ms ?? 0,
                available: status.playable === true,
              }}
            />
          </div>
        </div>
      </div>
    </header>
  );
}
