import { Play, Shuffle } from "lucide-react";
import type { Track } from "../model";
import type { TrackListing } from "../api/trackListing";
import { usePlayTracks } from "../hooks/usePlayTracks";
import { usePlayer } from "./PlaybackProvider";

export default function PlayShuffle({
  tracks,
  listing,
  label = "tracks",
  tone = "tint",
}: {
  tracks: Track[];
  listing?: TrackListing;
  label?: string;
  tone?: "tint" | "plain";
}) {
  const player = usePlayer();
  const play = usePlayTracks(tracks, listing);
  async function shuffle() {
    await play.playAll();
    if (!player.snapshot().shuffle) player.shuffle();
  }
  return (
    <>
      <button
        type="button"
        className={tone === "tint" ? "button tint-primary" : "button primary"}
        aria-label={`Play ${label}`}
        disabled={tracks.length === 0}
        onClick={() => void play.playAll()}
      >
        <Play size={17} fill="currentColor" />
        Play
      </button>
      <button
        type="button"
        className={tone === "tint" ? "button tint-secondary" : "button"}
        aria-label={`Shuffle ${label}`}
        disabled={tracks.length === 0}
        onClick={() => void shuffle()}
      >
        <Shuffle size={17} />
        Shuffle
      </button>
    </>
  );
}
