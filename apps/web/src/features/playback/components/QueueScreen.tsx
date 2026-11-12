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
import { ChevronRight, X } from "lucide-react";
import { useQueuedFirst } from "../hooks/useQueuedFirst";
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

const mac =
  typeof navigator !== "undefined" &&
  /Mac|iPhone|iPad/i.test(navigator.platform || navigator.userAgent);

export default function QueueScreen({
  docked = false,
  onClose,
}: {
  docked?: boolean;
  onClose?: () => void;
}) {
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
  const queuedFirst = useQueuedFirst(player, view);
  const firstOfNext = (view.currentIndex ?? -1) + 1;
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
    <div
      className={docked ? "queue-screen queue-screen-docked" : "queue-screen"}
    >
      {docked ? (
        <header className="queue-panel-head">
          <h2 id="queue-title" tabIndex={-1} data-queue-heading>
            Queue
          </h2>
          <button
            type="button"
            className="queue-clear"
            disabled={view.total === 0}
            onClick={() => setConfirming(true)}
          >
            Clear
          </button>
          <button
            type="button"
            className="queue-close"
            aria-label="Close queue"
            onClick={onClose}
          >
            <X size={18} aria-hidden="true" />
          </button>
        </header>
      ) : (
        <PageHeading
          title="Queue"
          description={`${count} · ${upNext.toLocaleString()} up next`}
        >
          <div className="queue-actions">
            <button
              type="button"
              className="queue-clear"
              disabled={view.total === 0}
              onClick={() => setConfirming(true)}
            >
              Clear
            </button>
            {onClose && (
              <button
                type="button"
                className="queue-close"
                aria-label="Close queue"
                onClick={onClose}
              >
                <X size={18} aria-hidden="true" />
              </button>
            )}
          </div>
        </PageHeading>
      )}
      <section
        aria-labelledby={docked ? "queue-now-dock" : "queue-now"}
        className="queue-now"
      >
        <h2 id={docked ? "queue-now-dock" : "queue-now"}>Now playing</h2>
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
            <PlayerTrackActions track={current} liking={false} />
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
          <summary>
            <ChevronRight size={16} strokeWidth={2.4} aria-hidden="true" />
            <span>{`Played (${played.toLocaleString()})`}</span>
          </summary>
          {playedOpen && <QueueList from={0} to={played} />}
        </details>
      )}
      {queuedFirst > 0 && (
        <section
          aria-labelledby={docked ? "queue-first-dock" : "queue-first"}
          className="queue-first"
        >
          <h2 id={docked ? "queue-first-dock" : "queue-first"}>
            Queued by you · plays first
          </h2>
          <QueueList from={firstOfNext} to={firstOfNext + queuedFirst} />
        </section>
      )}
      <section
        aria-labelledby={docked ? "queue-next-dock" : "queue-next"}
        className="queue-next"
      >
        <h2 id={docked ? "queue-next-dock" : "queue-next"}>Up next</h2>
        <QueueList from={firstOfNext + queuedFirst} />
        <p className="queue-hint">
          Tracks without a source are skipped. <kbd>{mac ? "⌥↑" : "Alt ↑"}</kbd>{" "}
          <kbd>{mac ? "⌥↓" : "Alt ↓"}</kbd>
        </p>
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
