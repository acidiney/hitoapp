import {
  memo,
  useCallback,
  useLayoutEffect,
  useRef,
  useState,
  type DragEvent,
} from "react";
import { usePlayer } from "./PlaybackProvider";
import QueueRow from "./QueueRow";
import { QUEUE_ROW, useQueueWindow } from "../hooks/useQueueWindow";

type Drag = { entry: string; index: number };

const QueueList = memo(function QueueList() {
  const player = usePlayer();
  const queue = useQueueWindow(player);
  const { view, start, end, scrolled } = queue;
  const scroller = useRef<HTMLDivElement>(null);
  const drag = useRef<Drag | null>(null);
  const [dropAt, setDropAt] = useState<number | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const latest = useRef({ view, queue, player });
  latest.current = { view, queue, player };

  useLayoutEffect(() => {
    const element = scroller.current;
    if (!element) return;
    const index = latest.current.view.currentIndex ?? 0;
    element.scrollTop = Math.max(0, (index - 1) * QUEUE_ROW);
    scrolled(element.scrollTop, element.clientHeight);
  }, [scrolled]);

  const place = useCallback(
    async (entry: string, at: number, title: string, position: number) => {
      const { view: now, queue: window, player: owner } = latest.current;
      const before = at >= now.total ? null : await window.idAt(at);
      owner.moveEntry(entry, before);
      setAnnouncement(`Moved ${title} to position ${position} of ${now.total}`);
    },
    [],
  );
  const move = useCallback(
    (entry: string, index: number, step: -1 | 1) => {
      const title =
        latest.current.queue.entryAt(index)?.recording.title ?? "track";
      void place(
        entry,
        step === -1 ? index - 1 : index + 2,
        title,
        index + step + 1,
      );
    },
    [place],
  );
  const select = useCallback(
    (entry: string) => void latest.current.player.selectEntry(entry),
    [],
  );
  const remove = useCallback(
    (entry: string) => latest.current.player.removeEntry(entry),
    [],
  );

  function row(event: DragEvent<HTMLElement>) {
    return (event.target as HTMLElement).closest<HTMLElement>("li[data-index]");
  }
  function edge(event: DragEvent<HTMLElement>, item: HTMLElement) {
    const box = item.getBoundingClientRect();
    const index = Number(item.dataset.index);
    return event.clientY > box.top + box.height / 2 ? index + 1 : index;
  }

  if (!view.total)
    return (
      <p className="queue-empty">
        Choose a track to start listening. Your queue stays with this device.
      </p>
    );
  const items = [];
  for (let index = start; index < end; index++) {
    const entry = queue.entryAt(index);
    if (!entry) continue;
    items.push(
      <QueueRow
        key={entry.id}
        entry={entry}
        index={index}
        total={view.total}
        current={entry.id === view.currentEntry}
        marker={
          dropAt === index
            ? "before"
            : dropAt === index + 1 && index === view.total - 1
              ? "after"
              : null
        }
        select={select}
        remove={remove}
        move={move}
      />,
    );
  }
  return (
    <>
      {view.pending > 0 && (
        <p className="queue-loading">
          {view.pending.toLocaleString()} more tracks are still loading.
        </p>
      )}
      {queue.error && (
        <div className="queue-empty" role="alert">
          <p>The queue could not be loaded. Try again.</p>
          <button className="text-link" onClick={queue.retry}>
            Retry queue
          </button>
        </div>
      )}
      <div
        className="queue-window"
        ref={scroller}
        onScroll={(event) =>
          scrolled(
            event.currentTarget.scrollTop,
            event.currentTarget.clientHeight,
          )
        }
      >
        <ol
          className="queue-tracks queue-virtual"
          style={{ height: view.total * QUEUE_ROW }}
          onDragStart={(event) => {
            const item = row(event);
            const entry = item && queue.entryAt(Number(item.dataset.index));
            if (!item || !entry) return;
            drag.current = {
              entry: entry.id,
              index: Number(item.dataset.index),
            };
            event.dataTransfer.effectAllowed = "move";
            event.dataTransfer.setData("text/plain", entry.id);
          }}
          onDragOver={(event) => {
            const item = row(event);
            if (!drag.current || !item) return;
            event.preventDefault();
            setDropAt(edge(event, item));
          }}
          onDrop={(event) => {
            const item = row(event);
            const dragged = drag.current;
            drag.current = null;
            setDropAt(null);
            if (!dragged || !item) return;
            event.preventDefault();
            const at = edge(event, item);
            if (at === dragged.index || at === dragged.index + 1) return;
            const title =
              queue.entryAt(dragged.index)?.recording.title ?? "track";
            void place(
              dragged.entry,
              at,
              title,
              at > dragged.index ? at : at + 1,
            );
          }}
          onDragEnd={() => {
            drag.current = null;
            setDropAt(null);
          }}
        >
          {items}
        </ol>
      </div>
      <p className="sr-only" aria-live="polite">
        {announcement}
      </p>
    </>
  );
});

export default QueueList;
