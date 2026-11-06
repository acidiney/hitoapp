import { useEffect, useRef, useSyncExternalStore } from "react";
import { ChevronDown } from "lucide-react";
import { usePlayer } from "./PlaybackProvider";
import { coverIdentity } from "../../artwork/model/coverIdentity";
import Artwork from "../../artwork/components/Artwork";
import { SeekBar, TransportButtons } from "./PlaybackControls";
import PlaybackStatus from "./PlaybackStatus";
import NowPlayingTabs from "./NowPlayingTabs";
import RouteLink from "../../../shared/ui/RouteLink";
import PlayerTrackActions from "./PlayerTrackActions";
import { artistHref } from "./TrackNames";
import { artistSearchLink } from "../hooks/useArtistLink";

export default function NowPlaying({
  account,
  close,
  openQueue,
}: {
  account: string;
  close: () => void;
  openQueue: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const player = usePlayer();
  const current = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().current,
  );
  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);
  return (
    <dialog
      ref={dialog}
      className="now-playing-dialog"
      aria-label="Now playing"
      onClose={close}
    >
      <header>
        <button
          className="icon-button"
          aria-label="Close now playing"
          onClick={() => dialog.current?.close()}
        >
          <ChevronDown size={20} />
        </button>
        <span>Now playing</span>
        <span aria-hidden="true" className="np-header-balance" />
      </header>
      <div className="np-body">
        <section className="np-track" aria-label="Track">
          <div className="np-cover">
            <Artwork
              release={current?.release_id ?? undefined}
              recording={current?.id}
              fallback={coverIdentity(current)}
            />
          </div>
          <div className="np-title">
            <div className="np-title-copy">
              <h1>{current?.title ?? "Ready when you are"}</h1>
              {current ? (
                <RouteLink
                  href={artistHref(current) ?? artistSearchLink(current.artist)}
                  onNavigate={close}
                >
                  {current.artist}
                </RouteLink>
              ) : (
                <p>Choose a track to start listening.</p>
              )}
            </div>
            {current && (
              <PlayerTrackActions track={current} onNavigate={close} />
            )}
          </div>
          <div className="np-seek">
            <SeekBar remaining />
          </div>
          <div className="np-transport">
            <TransportButtons />
          </div>
          <PlaybackStatus className="now-playing-status" />
        </section>
        <aside aria-label="Details" className="np-details">
          <NowPlayingTabs
            account={account}
            track={current}
            openQueue={openQueue}
            leave={close}
          />
        </aside>
      </div>
    </dialog>
  );
}
