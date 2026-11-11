import { memo, useCallback, useLayoutEffect, useRef, useState } from "react";
import type { PlaybackOccurrence } from "../model";
import { usePlayer } from "./PlaybackProvider";
import QueueRow from "./QueueRow";
import { insertionIndex, shiftFor } from "../hooks/queueDrag";
import { useQueueDrag } from "../hooks/useQueueDrag";
import { QUEUE_ROW, useQueueWindow } from "../hooks/useQueueWindow";

const QueueList = memo(function QueueList({
  from = 0,
  to,
}: {
  from?: number;
  to?: number;
}) {
  const player = usePlayer();
  const queue = useQueueWindow(player, from, to);
  const { view, start, end, scrolled } = queue;
  const scroller = useRef<HTMLDivElement>(null);
  const refocus = useRef<string | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const latest = useRef({ view, queue, player, from, to });
  latest.current = { view, queue, player, from, to };
  const last = Math.min(to ?? view.total, view.total);

  useLayoutEffect(() => {
    const element = scroller.current;
    if (!element || from > 0) return;
    const index = latest.current.view.currentIndex ?? 0;
    element.scrollTop = Math.max(0, (index - 1) * QUEUE_ROW);
    scrolled(element.scrollTop, element.clientHeight);
  }, [scrolled, from]);

  useLayoutEffect(() => {
    const entry = refocus.current;
    const list = scroller.current;
    if (!entry || !list) return;
    const grip = [...list.querySelectorAll<HTMLElement>("li")]
      .find((item) => item.dataset.entry === entry)
      ?.querySelector<HTMLButtonElement>(".queue-handle");
    if (!grip) return;
    const active = document.activeElement;
    if (!active || active === document.body) grip.focus();
    else refocus.current = null;
  });

  const place = useCallback(
    async (entry: string, at: number, title: string, position: number) => {
      const { view: now, queue: window, player: owner, from } = latest.current;
      const before = at >= now.total ? null : await window.idAt(at);
      owner.moveEntry(entry, before);
      setAnnouncement(
        `Moved ${title} to position ${position - from} of ${(latest.current.to ?? now.total) - from}`,
      );
    },
    [],
  );
  const move = useCallback(
    (entry: string, index: number, step: -1 | 1) => {
      const title =
        latest.current.queue.entryAt(index)?.recording.title ?? "track";
      refocus.current = entry;
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

  const { drag, begin } = useQueueDrag(
    scroller,
    { first: from, last, row: QUEUE_ROW },
    view.version,
    useCallback(
      (entry: string, origin: number, over: number) => {
        const title =
          latest.current.queue.entryAt(origin)?.recording.title ?? "track";
        void place(entry, insertionIndex(origin, over), title, over + 1);
      },
      [place],
    ),
    (index: number) => queue.entryAt(index)?.id,
  );

  if (!view.total)
    return (
      <p className="queue-empty">
        Choose a track to start listening. Your queue stays with this device.
      </p>
    );
  if (last <= from && (to !== undefined || view.pending === 0))
    return <p className="queue-empty">Nothing is queued after this track.</p>;
  const rowFor = (entry: PlaybackOccurrence, index: number) => (
    <QueueRow
      key={entry.id}
      entry={entry}
      index={index}
      first={from}
      total={last}
      current={entry.id === view.currentEntry}
      lifted={drag?.phase === "held" && drag.entry.id === entry.id}
      shift={drag ? shiftFor(index, drag.from, drag.over, QUEUE_ROW) : 0}
      select={select}
      remove={remove}
      move={move}
      grab={begin}
    />
  );
  const items = [];
  for (let index = start; index < end; index++) {
    const entry = queue.entryAt(index);
    if (entry) items.push(rowFor(entry, index));
  }
  if (drag && (drag.from < start || drag.from >= end))
    items.push(rowFor(drag.entry, drag.from));
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
        data-dragging={drag?.phase === "held" ? "" : undefined}
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
          style={{ height: Math.max(0, last - from) * QUEUE_ROW }}
          onKeyDown={(event) => {
            if (!event.altKey || !["ArrowUp", "ArrowDown"].includes(event.key))
              return;
            const item = (event.target as HTMLElement).closest<HTMLElement>(
              "li[data-index]",
            );
            const entry = item?.dataset.entry;
            if (!item || !entry) return;
            event.preventDefault();
            const index = Number(item.dataset.index);
            const step = event.key === "ArrowUp" ? -1 : 1;
            if (index + step < latest.current.from) return;
            const { view: now, to: end } = latest.current;
            if (index + step >= Math.min(end ?? now.total, now.total)) return;
            move(entry, index, step);
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
