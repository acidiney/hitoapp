import type { Schema } from "../../../shared/schema";
import { Play } from "lucide-react";
import { time } from "../../../shared/format";
import Artwork from "../../artwork/components/Artwork";
import Button from "../../../shared/ui/Button";
import { coverPaint } from "../../../shared/ui/coverArt";
import { oklchToHex } from "../../../shared/ui/coverColor";
import type { RadioState } from "../hooks/useArtistRadio";
import type { RadioPreview } from "../hooks/useRadioPreview";

const FIRST_UP = 3;

function mixLine(radio: Schema<"ArtistRadio">): string {
  const ready = radio.tracks.filter((track) => track.artist === radio.artist);
  const own = `${ready.length || radio.tracks.length} ready ${(ready.length || radio.tracks.length) === 1 ? "track" : "tracks"}`;
  if (radio.similar_artists === 0)
    return `Starts with ${own} of ${radio.artist}.`;
  return `Starts with ${own}, then ${radio.similar_artists} nearby ${radio.similar_artists === 1 ? "artist" : "artists"} in your library.`;
}

function Availability({ track }: { track: Schema<"Recording"> }) {
  if (track.available) return <>Ready</>;
  if (track.plays_from_services) return <>Ready · from your services</>;
  return <>No source</>;
}

export default function RadioHero({
  artist,
  mostOwned,
  preview,
  radio,
  onStart,
}: {
  artist: Schema<"ArtistIdentityGroup">;
  mostOwned: boolean;
  preview: RadioPreview;
  radio: RadioState;
  onStart: () => void;
}) {
  const name = artist.name.value;
  const busy = radio.pending === artist.id;
  const tint = oklchToHex(0.9, 0.04, coverPaint("artist", artist.id).hue);
  return (
    <section
      className="radio-hero"
      aria-label="Suggested radio"
      style={{ "--hero-tint": tint } as React.CSSProperties}
    >
      <span className="radio-hero-art" aria-hidden="true">
        <Artwork
          recording={artist.name.recording_id}
          fallback={{ id: artist.id, title: name, kind: "artist" }}
        />
      </span>
      <div className="radio-hero-copy">
        <span className="radio-hero-eyebrow">
          {mostOwned
            ? "Radio · because you own the most of this artist"
            : "Radio · from an artist in your library"}
        </span>
        <h2>{name} radio</h2>
        <p>
          {preview.status === "ready"
            ? mixLine(preview.radio)
            : `Starts with ${artist.recording_count} ${artist.recording_count === 1 ? "track" : "tracks"} from your library.`}
        </p>
        <div className="radio-hero-actions">
          <Button
            variant="action"
            disabled={radio.pending !== null}
            aria-busy={busy || undefined}
            onClick={onStart}
          >
            <Play size={16} fill="currentColor" aria-hidden="true" />
            {busy ? "Building your radio…" : "Start radio"}
          </Button>
        </div>
      </div>
      {preview.status === "ready" && preview.radio.tracks.length > 0 && (
        <div className="radio-hero-next">
          <span className="radio-hero-eyebrow">First up</span>
          <ol>
            {preview.radio.tracks.slice(0, FIRST_UP).map((track) => (
              <li key={track.id}>
                <span className="radio-hero-cover" aria-hidden="true">
                  <Artwork
                    release={track.release_id ?? undefined}
                    recording={track.id}
                    fallback={{
                      id: track.release_id ?? track.id,
                      title: track.album ?? track.title,
                      artist: track.artist,
                    }}
                  />
                </span>
                <span className="radio-hero-track">
                  <span>{track.title}</span>
                  <span>
                    <Availability track={track} />
                  </span>
                </span>
                <span className="radio-hero-time">
                  {time(track.duration_ms / 1000)}
                </span>
              </li>
            ))}
          </ol>
        </div>
      )}
    </section>
  );
}
