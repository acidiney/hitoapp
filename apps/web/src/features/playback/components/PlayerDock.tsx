import {
  useCallback,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type CSSProperties,
} from "react";
import { ChevronUp, ListOrdered, Volume2 } from "lucide-react";
import { coverIdentity } from "../../artwork/model/coverIdentity";
import { usePlayer } from "./PlaybackProvider";
import PlaybackControls from "./PlaybackControls";
import Artwork from "../../artwork/components/Artwork";
import NowPlaying from "./NowPlaying";
import PlaybackStatus from "./PlaybackStatus";
import SpeedControl from "./SpeedControl";
import { log } from "../../../shared/log";
import RouteLink, { visitFromPlayer } from "../../../shared/ui/RouteLink";
import { trackLink, useArtistLink } from "../hooks/useArtistLink";

export default function PlayerDock({
  account,
  queueActive = false,
}: {
  account: string;
  queueActive?: boolean;
}) {
  const player = usePlayer();
  const current = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().current,
  );
  const controlsAvailable = useSyncExternalStore(player.subscribe, () => {
    const state = player.snapshot();
    return state.connection === "ready" && state.controlsAvailable !== false;
  });
  const queueTotal = useSyncExternalStore(
    player.subscribe,
    () => player.queueSnapshot().total,
  );
  const volume = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().volume,
  );
  const artistHref = useArtistLink(account, current);
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
    setExpanded(true);
  }, []);
  function showQueue() {
    log.debug("player.queue", { enabled: true });
    visitFromPlayer("/queue");
  }
  return (
    <>
      {expanded && (
        <NowPlaying
          account={account}
          close={closeExpanded}
          openQueue={closeExpanded}
        />
      )}
      <footer className="player" aria-label="Audio player" hidden={expanded}>
        <div className="now-playing">
          <button
            className="dock-artwork"
            aria-label="Expand player"
            disabled={!current}
            onClick={(event) => openExpanded(event.currentTarget)}
          >
            <Artwork
              release={current?.release_id ?? undefined}
              recording={current?.id}
              fallback={coverIdentity(current)}
            />
          </button>
          <span>
            {current ? (
              <strong>
                <RouteLink className="player-link" href={trackLink(current)}>
                  {current.title}
                </RouteLink>
              </strong>
            ) : (
              <strong>Ready when you are</strong>
            )}
            {current && artistHref ? (
              <small>
                <RouteLink className="player-link" href={artistHref}>
                  {current.artist}
                </RouteLink>
              </small>
            ) : (
              <small>Choose a track from your library</small>
            )}
          </span>
        </div>
        <PlaybackControls />
        <div className="player-tools">
          <SpeedControl available={controlsAvailable} />
          <Volume2 className="volume-icon" size={18} />
          <input
            className="volume"
            aria-label="Volume"
            type="range"
            min="0"
            max="1"
            step="0.05"
            style={{ "--fill": `${volume * 100}%` } as CSSProperties}
            value={volume}
            onChange={(event) => player.volume(+event.target.value)}
          />
          <button
            className="icon-button expand-player"
            aria-label="Open now playing"
            disabled={!current}
            onClick={(event) => openExpanded(event.currentTarget)}
          >
            <ChevronUp size={16} />
          </button>
          <button
            ref={queueButton}
            className={`icon-button queue-button ${queueActive ? "selected" : ""}`}
            aria-label="Open queue"
            aria-current={queueActive ? "page" : undefined}
            onClick={showQueue}
          >
            <ListOrdered size={16} />
            <span className="queue-label">
              Queue{queueTotal > 0 && ` · ${queueTotal}`}
            </span>
          </button>
        </div>
        <PlaybackStatus />
      </footer>
    </>
  );
}
