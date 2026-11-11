import { Play, Shuffle } from "lucide-react";
import type { Track } from "../model";
import type { TrackListing } from "../api/trackListing";
import { usePlayTracks } from "../hooks/usePlayTracks";

type Tone = "tint" | "plain" | "header";

const PLAY_CLASS: Record<Tone, string> = {
  tint: "button tint-primary",
  plain: "button primary",
  header: "button primary",
};

export default function PlayShuffle({
  tracks,
  listing,
  label = "tracks",
  tone = "tint",
  playText,
}: {
  tracks: Track[];
  listing?: TrackListing;
  label?: string;
  tone?: Tone;
  playText?: string;
}) {
  const play = usePlayTracks(tracks, listing);
  return (
    <>
      <button
        type="button"
        className={PLAY_CLASS[tone]}
        aria-label={playText ? undefined : `Play ${label}`}
        disabled={tracks.length === 0}
        onClick={() => void play.playAll()}
      >
        <Play size={17} fill="currentColor" />
        {playText ?? "Play"}
      </button>
      <button
        type="button"
        className={
          tone === "tint"
            ? "button tint-secondary"
            : tone === "header"
              ? "button icon-round"
              : "button"
        }
        aria-label={`Shuffle ${label}`}
        disabled={tracks.length === 0}
        onClick={() => void play.shuffleAll()}
      >
        <Shuffle size={tone === "header" ? 18 : 17} />
        {tone !== "header" && "Shuffle"}
      </button>
    </>
  );
}
