import { useEffect, useState, useSyncExternalStore } from "react";
import { usePlayer } from "./PlaybackProvider";
import QueueList from "./QueueList";
import Artwork from "../../artwork/components/Artwork";
import { coverIdentity } from "../../artwork/model/coverIdentity";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import PageHeading from "../../../shared/ui/PageHeading";
import { time } from "../../../shared/format";
import { log } from "../../../shared/log";
import { X } from "lucide-react";
import PlayerTrackActions from "./PlayerTrackActions";
import type { Player, PlaybackOccurrence, QueueView } from "../model";

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

function useCurrentEntry(player: Player, view: QueueView) {
  const [entry, setEntry] = useState<PlaybackOccurrence | null>(null);
  useEffect(() => {
    if (view.currentIndex === null) {
      setEntry(null);
      return;
    }
    const controller = new AbortController();
    player
      .queuePage(view.currentIndex, 1, controller.signal)
      .then((page) => {
        if (!controller.signal.aborted) setEntry(page.entries[0] ?? null);
      })
      .catch(() => {});
    return () => controller.abort();
  }, [player, view.version, view.currentIndex, view.currentEntry]);
  return entry;
}

export default function QueueScreen() {
  const player = usePlayer();
  const view = useSyncExternalStore(player.subscribe, player.queueSnapshot);
  const playing = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().current,
  );
  const entry = useCurrentEntry(player, view);
  const current = entry?.recording ?? playing;
  const currentId = entry?.id ?? view.currentEntry;
  const played = view.currentIndex ?? 0;
  const upNext = Math.max(0, view.total - (view.currentIndex ?? -1) - 1);
  const [playedOpen, setPlayedOpen] = useState(false);
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
        description={`${count} · ${upNext.toLocaleString()} up next`}
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
            <PlayerTrackActions track={current} />
            {currentId && (
              <button
                type="button"
                className="icon-button"
                aria-label={`Remove ${current.title} from queue`}
                onClick={() => player.removeEntry(currentId)}
              >
                <X size={16} />
              </button>
            )}
          </div>
        ) : (
          <p className="queue-empty">Nothing is playing.</p>
        )}
      </section>
      {played > 0 && (
        <details
          className="queue-played"
          onToggle={(event) => setPlayedOpen(event.currentTarget.open)}
        >
          <summary>{`Played (${played.toLocaleString()})`}</summary>
          {playedOpen && <QueueList from={0} to={played} />}
        </details>
      )}
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
            Playback stops and the queue ({count}) is emptied on this device.
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
