import { useSyncExternalStore } from "react";
import {
  Pause,
  Play,
  Repeat,
  Shuffle,
  SkipBack,
  SkipForward,
} from "lucide-react";
import { time } from "../format";
import { usePlayer } from "./PlaybackProvider";

export default function PlaybackControls() {
  const player = usePlayer();
  const state = useSyncExternalStore(player.subscribe, player.snapshot);
  const position = useSyncExternalStore(
    player.subscribePosition,
    player.positionSnapshot,
  );
  return (
    <div className="transport">
      <div className="transport-buttons">
        <button
          className={`icon-button ${state.shuffle ? "selected" : ""}`}
          disabled={!state.current}
          aria-label="Shuffle"
          aria-pressed={state.shuffle}
          onClick={() => player.shuffle()}
        >
          <Shuffle size={17} />
        </button>
        <button
          className="icon-button"
          disabled={!state.current}
          aria-label="Previous track"
          onClick={() => void player.previous()}
        >
          <SkipBack size={19} />
        </button>
        <button
          className="play-button"
          disabled={!state.current || state.loading}
          aria-label={state.playing ? "Pause" : "Play"}
          onClick={() => void player.toggle()}
        >
          {state.playing ? <Pause size={21} /> : <Play size={21} />}
        </button>
        <button
          className="icon-button"
          disabled={!state.current}
          aria-label="Next track"
          onClick={() => void player.next()}
        >
          <SkipForward size={19} />
        </button>
        <button
          className={`icon-button ${state.repeat !== "off" ? "selected" : ""}`}
          disabled={!state.current}
          aria-label={`Repeat: ${state.repeat}`}
          onClick={() => player.repeat()}
        >
          <Repeat size={17} />
          {state.repeat === "one" && <sup>1</sup>}
        </button>
      </div>
      <div className="seek">
        <span>{time(position.position)}</span>
        <input
          aria-label="Seek"
          type="range"
          min="0"
          max={position.duration || 1}
          step="0.1"
          value={Math.min(position.position, position.duration || 0)}
          disabled={!state.current}
          onChange={(event) => player.seek(+event.target.value)}
        />
        <span>{time(position.duration)}</span>
      </div>
    </div>
  );
}
