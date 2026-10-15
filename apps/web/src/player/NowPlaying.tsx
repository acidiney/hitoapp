import { useEffect, useRef, useSyncExternalStore } from "react";
import { ArrowDownLeft, X } from "lucide-react";
import { usePlayer } from "./PlaybackProvider";
import Artwork from "../components/Artwork";
import QueueTracks from "./QueueTracks";
import PlaybackControls from "./PlaybackControls";
import PlaybackStatus from "./PlaybackStatus";

export default function NowPlaying({ close }: { close: () => void }) {
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
        <span>Now playing</span>
        <button
          className="icon-button"
          aria-label="Close now playing"
          onClick={() => dialog.current?.close()}
        >
          <X size={21} />
        </button>
      </header>
      <div className="now-playing-body">
        <section className="now-playing-focus">
          <Artwork recording={current?.id} />
          <div className="now-playing-caption">
            <span>From your library</span>
            <h1>{current?.title ?? "Ready when you are"}</h1>
            <p>{current?.artist ?? "Choose a track to start listening."}</p>
          </div>
        </section>
        <aside
          className="now-playing-queue"
          aria-label="Expanded playback queue"
        >
          <div className="section-heading">
            <h2>Up next</h2>
            <ArrowDownLeft size={18} />
          </div>
          <QueueTracks />
        </aside>
      </div>
      <div className="now-playing-controls">
        <PlaybackStatus className="now-playing-status" />
        <PlaybackControls />
      </div>
    </dialog>
  );
}
