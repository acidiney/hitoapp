import {
  useCallback,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type CSSProperties,
} from "react";
import { ChevronUp, ListMusic, Volume2, VolumeX } from "lucide-react";
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
import { useMute } from "../hooks/useMute";
import { onNowPlayingRequest } from "../nowPlayingRequests";
import { rememberNowPlayingOrigin } from "../nowPlayingOrigin";
import PlayerTrackActions from "./PlayerTrackActions";
import { ArtistNames, artistHref as knownArtistHref } from "./TrackNames";

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
  const knownArtist = current ? knownArtistHref(current) : null;
  const matchedArtist = useArtistLink(account, knownArtist ? null : current);
  const artistHref = knownArtist ?? matchedArtist;
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
  useEffect(
    () =>
      onNowPlayingRequest(() => {
        if (current && !expanded) {
          expandedOpener.current = null;
          rememberNowPlayingOrigin();
          setExpanded(true);
        }
      }),
    [current, expanded],
  );
  const closeExpanded = useCallback(() => {
    log.debug("player.expanded", { enabled: false });
    setExpanded(false);
  }, []);
  const openExpanded = useCallback((opener: HTMLButtonElement) => {
    log.debug("player.expanded", { enabled: true });
    expandedOpener.current = opener;
    rememberNowPlayingOrigin();
    setExpanded(true);
  }, []);
  const toggleMute = useMute();
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
                <ArtistNames
                  artist={current.artist}
                  artistHref={artistHref}
                  credited={current.credited_artists}
                  routed={{ className: "player-link" }}
                />
              </small>
            ) : (
              <small>Choose a track from your library</small>
            )}
          </span>
          {current && <PlayerTrackActions track={current} />}
        </div>
        <PlaybackControls />
        <div className="player-tools">
          <button
            ref={queueButton}
            className={`icon-button queue-button ${queueActive ? "selected" : ""}`}
            aria-label="Open queue"
            aria-current={queueActive ? "page" : undefined}
            onClick={showQueue}
          >
            <ListMusic size={18} aria-hidden="true" />
            <span className="queue-label">
              Queue{queueTotal > 0 && ` · ${queueTotal}`}
            </span>
          </button>
          <SpeedControl available={controlsAvailable} />
          <button
            className="icon-button mute-button"
            aria-label="Mute"
            aria-pressed={volume === 0}
            onClick={toggleMute}
          >
            {volume === 0 ? (
              <VolumeX size={18} aria-hidden="true" />
            ) : (
              <Volume2 size={18} aria-hidden="true" />
            )}
          </button>
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
            <ChevronUp size={18} aria-hidden="true" />
          </button>
        </div>
        <PlaybackStatus />
      </footer>
    </>
  );
}
