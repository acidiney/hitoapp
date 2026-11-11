import { useCallback, useRef, type PointerEvent, type MouseEvent } from "react";

const HOLD_MS = 500;
const MOVE_PX = 10;

export function useLongPress(onLongPress: () => void) {
  const timer = useRef<number | null>(null);
  const origin = useRef<{ x: number; y: number } | null>(null);
  const fired = useRef(false);
  const latest = useRef(onLongPress);
  latest.current = onLongPress;
  const cancel = useCallback(() => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = null;
    origin.current = null;
  }, []);
  return {
    onPointerDown(event: PointerEvent) {
      if (event.pointerType !== "touch") return;
      fired.current = false;
      origin.current = { x: event.clientX, y: event.clientY };
      timer.current = window.setTimeout(() => {
        timer.current = null;
        fired.current = true;
        latest.current();
      }, HOLD_MS);
    },
    onPointerMove(event: PointerEvent) {
      const start = origin.current;
      if (!start) return;
      if (
        Math.hypot(event.clientX - start.x, event.clientY - start.y) > MOVE_PX
      )
        cancel();
    },
    onPointerUp: cancel,
    onPointerCancel: cancel,
    onClickCapture(event: MouseEvent) {
      if (!fired.current) return;
      fired.current = false;
      event.preventDefault();
      event.stopPropagation();
    },
    onContextMenu(event: MouseEvent) {
      if (timer.current !== null || fired.current) event.preventDefault();
    },
  };
}
