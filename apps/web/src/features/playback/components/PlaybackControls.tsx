import { useSyncExternalStore, type CSSProperties } from "react";
import {
  Pause,
  Play,
  Repeat,
  Shuffle,
  SkipBack,
  SkipForward,
} from "lucide-react";
import { time } from "../../../shared/format";
import { usePlayer } from "./PlaybackProvider";

export default function PlaybackControls() {
  const player = usePlayer();
  const state = useSyncExternalStore(player.subscribe, player.snapshot);
  const position = useSyncExternalStore(
    player.subscribePosition,
    player.positionSnapshot,
  );
  const playing = state.desiredPlaying ?? state.playing;
  const selected =
    player.mode === "native"
      ? !!player.queueSnapshot().currentEntry
      : !!state.current;
  const queued = player.queueSnapshot().total > 0;
  return (
    <div className="transport">
      <div className="transport-buttons">
        <button
          className={`icon-button transport-optional ${state.shuffle ? "selected" : ""}`}
          disabled={
            !queued || !player.supports.shuffle || state.connection !== "ready"
          }
          title={
            !player.supports.shuffle
              ? "Shuffle is not available in the desktop player yet"
              : undefined
          }
          aria-label="Shuffle"
          aria-pressed={state.shuffle}
          onClick={() => player.shuffle()}
        >
          <Shuffle size={16} />
        </button>
        <button
          className="icon-button transport-optional"
          disabled={
            !(state.canPrevious ?? selected) ||
            !player.supports.previous ||
            state.connection !== "ready"
          }
          title={
            !player.supports.previous
              ? "Previous track is not available in the desktop player yet"
              : undefined
          }
          aria-label="Previous track"
          onClick={() => void player.previous()}
        >
          <SkipBack size={18} fill="currentColor" />
        </button>
        <button
          className="play-button"
          disabled={
            !selected ||
            (player.mode === "browser" && state.loading) ||
            state.connection !== "ready"
          }
          aria-label={playing ? "Pause" : "Play"}
          onClick={() => void player.toggle()}
        >
          {playing ? <Pause size={16} fill="currentColor" /> : <Play size={16} fill="currentColor" />}
        </button>
        <button
          className="icon-button"
          disabled={!selected || state.connection !== "ready"}
          aria-label="Next track"
          onClick={() => void player.next()}
        >
          <SkipForward size={18} fill="currentColor" />
        </button>
        <button
          className={`icon-button transport-optional ${state.repeat !== "off" ? "selected" : ""}`}
          disabled={
            !queued || !player.supports.repeat || state.connection !== "ready"
          }
          title={
            !player.supports.repeat
              ? "Repeat is not available in the desktop player yet"
              : undefined
          }
          aria-label={`Repeat: ${state.repeat}`}
          onClick={() => player.repeat()}
        >
          <Repeat size={16} />
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
          style={
            {
              "--fill": `${position.duration ? Math.min(100, (position.position / position.duration) * 100) : 0}%`,
            } as CSSProperties
          }
          value={Math.min(position.position, position.duration || 0)}
          disabled={!selected || state.connection !== "ready"}
          onChange={(event) => player.seek(+event.target.value)}
        />
        <span>{time(position.duration)}</span>
      </div>
    </div>
  );
}
