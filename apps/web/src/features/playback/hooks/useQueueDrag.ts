import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
  type RefObject,
} from "react";
import type { PlaybackOccurrence } from "../model";
import { clampTravel, dropSlot, edgeSpeed, SLIDE_MS } from "./queueDrag";

export type QueueDrag = {
  entry: PlaybackOccurrence;
  from: number;
  over: number;
  phase: "held" | "dropped";
};

type Geometry = { first: number; last: number; row: number };
type Session = {
  entry: PlaybackOccurrence;
  from: number;
  over: number;
  pointer: number;
  pointerY: number;
  startY: number;
  frame: number;
};

const SETTLE_TIMEOUT_MS = SLIDE_MS * 6;

export function useQueueDrag(
  scroller: RefObject<HTMLElement | null>,
  geometry: Geometry,
  version: number,
  commit: (entry: string, from: number, over: number) => void,
  settledAt: (index: number) => string | undefined,
) {
  const [drag, setDrag] = useState<QueueDrag | null>(null);
  const session = useRef<Session | null>(null);
  const stop = useRef<(() => void) | null>(null);
  const latest = useRef({ geometry, commit });
  latest.current = { geometry, commit };

  const rowOf = useCallback(
    (entry: string) => {
      const items = scroller.current?.querySelectorAll<HTMLElement>("li");
      return items && [...items].find((item) => item.dataset.entry === entry);
    },
    [scroller],
  );

  const apply = useCallback(() => {
    const active = session.current;
    const element = scroller.current;
    if (!active || !element) return;
    const { first, last, row } = latest.current.geometry;
    const contentY =
      active.pointerY - element.getBoundingClientRect().top + element.scrollTop;
    const travel = clampTravel(
      contentY - active.startY,
      active.from,
      first,
      last,
      row,
    );
    rowOf(active.entry.id)?.style.setProperty("--drag-y", `${travel}px`);
    const over = dropSlot(active.from, travel, first, last, row);
    if (over === active.over) return;
    active.over = over;
    setDrag((old) => (old?.phase === "held" ? { ...old, over } : old));
  }, [rowOf, scroller]);

  const finish = useCallback(
    (dropped: boolean) => {
      const active = session.current;
      if (!active) return;
      stop.current?.();
      stop.current = null;
      session.current = null;
      cancelAnimationFrame(active.frame);
      const { row } = latest.current.geometry;
      const moved = dropped && active.over !== active.from;
      const element = rowOf(active.entry.id);
      if (!moved) {
        element?.style.setProperty("--drag-y", "0px");
        setDrag(null);
        return;
      }
      element?.style.setProperty(
        "--drag-y",
        `${(active.over - active.from) * row}px`,
      );
      setDrag({
        entry: active.entry,
        from: active.from,
        over: active.over,
        phase: "dropped",
      });
      latest.current.commit(active.entry.id, active.from, active.over);
    },
    [rowOf],
  );

  const begin = useCallback(
    (
      event: ReactPointerEvent<HTMLElement>,
      entry: PlaybackOccurrence,
      index: number,
    ) => {
      const element = scroller.current;
      if (session.current || !element) return;
      if (event.pointerType === "mouse" && event.button !== 0) return;
      try {
        event.currentTarget.setPointerCapture(event.pointerId);
      } catch {
        void 0;
      }
      const started: Session = {
        entry,
        from: index,
        over: index,
        pointer: event.pointerId,
        pointerY: event.clientY,
        startY:
          event.clientY -
          element.getBoundingClientRect().top +
          element.scrollTop,
        frame: 0,
      };
      session.current = started;
      const moved = (move: PointerEvent) => {
        if (move.pointerId !== started.pointer) return;
        started.pointerY = move.clientY;
        apply();
      };
      const released = (up: PointerEvent) => {
        if (up.pointerId === started.pointer) finish(up.type === "pointerup");
      };
      const keyed = (key: KeyboardEvent) => {
        if (key.key !== "Escape") return;
        key.preventDefault();
        key.stopPropagation();
        finish(false);
      };
      const tick = () => {
        const target = scroller.current;
        if (session.current !== started || !target) return;
        const box = target.getBoundingClientRect();
        const speed = edgeSpeed(
          started.pointerY,
          Math.max(box.top, 0),
          Math.min(box.bottom, window.innerHeight),
        );
        if (speed) {
          const before = target.scrollTop;
          target.scrollTop += speed;
          if (target.scrollTop === before) window.scrollBy(0, speed);
          apply();
        }
        started.frame = requestAnimationFrame(tick);
      };
      window.addEventListener("pointermove", moved);
      window.addEventListener("pointerup", released);
      window.addEventListener("pointercancel", released);
      window.addEventListener("keydown", keyed, true);
      stop.current = () => {
        window.removeEventListener("pointermove", moved);
        window.removeEventListener("pointerup", released);
        window.removeEventListener("pointercancel", released);
        window.removeEventListener("keydown", keyed, true);
      };
      started.frame = requestAnimationFrame(tick);
      setDrag({ entry, from: index, over: index, phase: "held" });
    },
    [apply, finish, scroller],
  );

  useEffect(() => {
    if (session.current) finish(false);
  }, [version, finish]);

  useEffect(
    () => () => {
      stop.current?.();
      if (session.current) cancelAnimationFrame(session.current.frame);
      session.current = null;
    },
    [],
  );

  useEffect(() => {
    if (drag?.phase !== "dropped") return;
    const timer = window.setTimeout(() => {
      rowOf(drag.entry.id)?.style.removeProperty("--drag-y");
      setDrag(null);
    }, SETTLE_TIMEOUT_MS);
    return () => window.clearTimeout(timer);
  }, [drag, rowOf]);

  const settled =
    drag?.phase === "dropped" && settledAt(drag.over) === drag.entry.id;
  useLayoutEffect(() => {
    if (!settled || !drag) return;
    rowOf(drag.entry.id)?.style.removeProperty("--drag-y");
    setDrag(null);
  }, [settled, drag, rowOf]);

  return { drag, begin };
}
