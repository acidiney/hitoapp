import { useState, useSyncExternalStore } from "react";
import { usePlayer } from "./PlaybackProvider";
import QueueList from "./QueueList";
import Artwork from "../../artwork/components/Artwork";
import { coverIdentity } from "../../artwork/coverIdentity";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import PageHeading from "../../../shared/ui/PageHeading";
import { time } from "../../../shared/format";
import { log } from "../../../shared/log";

function NowClock() {
  const player = usePlayer();
  const position = useSyncExternalStore(
    player.subscribePosition,
    player.positionSnapshot,
  );
  return (
    <span className="mono">
      {`${time(position.position)} / ${time(position.duration)}`}
    </span>
  );
}

export default function QueueScreen() {
  const player = usePlayer();
  const view = useSyncExternalStore(player.subscribe, player.queueSnapshot);
  const current = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().current,
  );
  const [confirming, setConfirming] = useState(false);
  const count = `${view.total.toLocaleString()} ${view.total === 1 ? "track" : "tracks"}`;
  function clear() {
    log.info("queue.clear_confirmed", { count: view.total });
    player.clear();
    setConfirming(false);
  }
  return (
    <div className="queue-screen">
      <PageHeading
        title="Queue"
        description={
          view.pending > 0
            ? `${count} · ${view.pending.toLocaleString()} more loading`
            : count
        }
      >
        <div className="queue-actions">
          <Button
            variant="destructive"
            disabled={view.total === 0}
            onClick={() => setConfirming(true)}
          >
            Clear
          </Button>
        </div>
      </PageHeading>
      <section aria-labelledby="queue-now" className="queue-now">
        <h2 id="queue-now">Now playing</h2>
        {current ? (
          <div className="queue-now-row">
            <Artwork
              release={current.release_id ?? undefined}
              recording={current.id}
              fallback={coverIdentity(current)}
            />
            <span>
              <strong>{current.title}</strong>
              <small>{current.artist}</small>
            </span>
            <NowClock />
          </div>
        ) : (
          <p className="queue-empty">Nothing is playing.</p>
        )}
      </section>
      <section aria-labelledby="queue-next" className="queue-next">
        <div className="queue-next-head">
          <h2 id="queue-next">Up next</h2>
          <span>Drag by the handle, or use the arrows, to reorder</span>
        </div>
        <QueueList from={(view.currentIndex ?? -1) + 1} />
      </section>
      {confirming && (
        <Dialog title="Clear the queue?" onClose={() => setConfirming(false)}>
          <p className="muted">
            Playback stops and {count} are removed from this device’s queue.
            Your library and playlists are not changed.
          </p>
          <div className="dialog-actions">
            <Button onClick={() => setConfirming(false)}>Cancel</Button>
            <Button variant="destructive" onClick={clear}>
              Clear queue
            </Button>
          </div>
        </Dialog>
      )}
    </div>
  );
}
