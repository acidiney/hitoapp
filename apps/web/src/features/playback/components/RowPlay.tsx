import { Play } from "lucide-react";
import type { RowPlayState } from "../hooks/useNowPlaying";
import { PlayingBars, PlayingStatus } from "./PlayingBars";

export default function RowPlay({
  title,
  number,
  disabled,
  state = null,
  onPlay,
}: {
  title: string;
  number: string | number;
  disabled?: boolean;
  state?: RowPlayState;
  onPlay: () => void;
}) {
  return (
    <>
      <button
        className="row-play icon-button"
        aria-label={`Play ${title}`}
        data-state={state ?? undefined}
        disabled={disabled}
        onClick={onPlay}
      >
        <span>{state ? <PlayingBars state={state} /> : number}</span>
        <Play size={15} />
      </button>
      {state && <PlayingStatus state={state} />}
    </>
  );
}
