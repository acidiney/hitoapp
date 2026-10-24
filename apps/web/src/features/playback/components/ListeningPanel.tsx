import { useSyncExternalStore } from "react";
import { ArrowUpRight, Headphones } from "lucide-react";
import { usePlayer } from "./PlaybackProvider";
import Artwork from "../../artwork/components/Artwork";
import QueueTracks from "./QueueTracks";

export default function ListeningPanel({
  expand,
  openQueue,
}: {
  expand: (opener: HTMLButtonElement) => void;
  openQueue: () => void;
}) {
  const player = usePlayer();
  const current = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().current,
  );
  return (
    <aside className="listening-panel" aria-label="Listening panel">
      <div className="section-heading">
        <h2>Now playing</h2>
        <button
          className="icon-button"
          aria-label="Expand now playing"
          onClick={(event) => expand(event.currentTarget)}
          disabled={!current}
        >
          <ArrowUpRight size={18} />
        </button>
      </div>
      <button
        className="listening-cover"
        onClick={(event) => expand(event.currentTarget)}
        disabled={!current}
        aria-label="View now playing artwork"
      >
        <Artwork
          release={current?.release_id ?? undefined}
          recording={current?.id}
        />
      </button>
      <div className="listening-title">
        <h3>{current?.title ?? "Make yourself at home"}</h3>
        <p>{current?.artist ?? "Pick something from your library to begin."}</p>
      </div>
      <section className="listening-next">
        <div className="section-heading">
          <h2>Coming up</h2>
          <button className="text-link" onClick={openQueue}>
            View queue
          </button>
        </div>
        <QueueTracks preview />
      </section>
      <div className="listening-device">
        <Headphones size={18} />
        <span>
          {player.mode === "native" ? "This device" : "This browser"}
          <small>Your listening session</small>
        </span>
      </div>
    </aside>
  );
}
