import {
  useCallback,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { ListOrdered, Maximize2, Volume2 } from "lucide-react";
import { usePlayer } from "./PlaybackProvider";
import PlaybackControls from "./PlaybackControls";
import PlaybackQueue from "./PlaybackQueue";
import Artwork from "../components/Artwork";
import ListeningPanel from "./ListeningPanel";
import NowPlaying from "./NowPlaying";
import PlaybackStatus from "./PlaybackStatus";
import { log } from "../log";

export default function PlayerDock({
  showListeningPanel = false,
}: {
  showListeningPanel?: boolean;
}) {
  const player = usePlayer();
  const current = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().current,
  );
  const speed = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().speed,
  );
  const [queueOpen, setQueueOpen] = useState(false);
  const volume = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().volume,
  );
  const [expanded, setExpanded] = useState(false);
  const expandedOpener = useRef<HTMLButtonElement | null>(null);
  const queueButton = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (expanded) return;
    const opener = expandedOpener.current;
    expandedOpener.current = null;
    if (!opener) return;
    if (opener.isConnected && !opener.disabled)
      opener.focus({ preventScroll: true });
    if (document.activeElement !== opener)
      queueButton.current?.focus({ preventScroll: true });
  }, [expanded]);
  const closeExpanded = useCallback(() => {
    log.debug("player.expanded", { enabled: false });
    setExpanded(false);
  }, []);
  const openExpanded = useCallback((opener: HTMLButtonElement) => {
    log.debug("player.expanded", { enabled: true });
    expandedOpener.current = opener;
    setQueueOpen(false);
    setExpanded(true);
  }, []);
  const closeQueue = useCallback(() => {
    log.debug("player.queue", { enabled: false });
    setQueueOpen(false);
  }, []);
  function showQueue(enabled: boolean) {
    log.debug("player.queue", { enabled });
    setQueueOpen(enabled);
  }
  return (
    <>
      {showListeningPanel && !queueOpen && (
        <ListeningPanel
          expand={openExpanded}
          openQueue={() => showQueue(true)}
        />
      )}
      {expanded && <NowPlaying close={closeExpanded} />}
      {queueOpen && <PlaybackQueue close={closeQueue} />}
      <footer className="player" aria-label="Audio player" hidden={expanded}>
        <div className="now-playing">
          <button
            className="dock-artwork"
            aria-label="Expand player"
            disabled={!current}
            onClick={(event) => openExpanded(event.currentTarget)}
          >
            <Artwork recording={current?.id} />
          </button>
          <span>
            <strong>{current?.title ?? "Ready when you are"}</strong>
            <small>
              {current?.artist ?? "Choose a track from your library"}
            </small>
          </span>
        </div>
        <PlaybackControls />
        <div className="player-tools">
          <select
            aria-label="Playback speed"
            value={speed}
            disabled={!player.supports.speed}
            title={
              !player.supports.speed
                ? "Playback speed is not available in the desktop player yet"
                : undefined
            }
            onChange={(event) => player.speed(+event.target.value)}
          >
            {[0.5, 0.75, 1, 1.25, 1.5, 2].map((speed) => (
              <option key={speed} value={speed}>
                {speed}×
              </option>
            ))}
          </select>
          <Volume2 size={18} />
          <input
            className="volume"
            aria-label="Volume"
            type="range"
            min="0"
            max="1"
            step="0.05"
            value={volume}
            onChange={(event) => player.volume(+event.target.value)}
          />
          <button
            className="icon-button expand-player"
            aria-label="Open now playing"
            disabled={!current}
            onClick={(event) => openExpanded(event.currentTarget)}
          >
            <Maximize2 size={17} />
          </button>
          <button
            ref={queueButton}
            className={`icon-button ${queueOpen ? "selected" : ""}`}
            aria-label="Open queue"
            aria-expanded={queueOpen}
            onClick={() => showQueue(!queueOpen)}
          >
            <ListOrdered size={20} />
          </button>
        </div>
        <PlaybackStatus />
      </footer>
    </>
  );
}
