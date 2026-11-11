import { useEffect, useRef, type KeyboardEvent } from "react";
import {
  QUEUE_PANEL_ID,
  setQueuePanelOpen,
  useQueuePanelOpen,
} from "../queuePanelState";
import QueueScreen from "./QueueScreen";

export const QUEUE_TOGGLE_ATTRIBUTE = "data-queue-toggle";

export default function QueuePanel() {
  const open = useQueuePanelOpen();
  const panel = useRef<HTMLElement>(null);
  const first = useRef(true);

  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    const element = panel.current;
    if (!element) return;
    if (open) {
      element
        .querySelector<HTMLElement>("[data-queue-heading]")
        ?.focus({ preventScroll: true });
      return;
    }
    const active = document.activeElement;
    if (!active || active === document.body || element.contains(active))
      document
        .querySelector<HTMLElement>(`[${QUEUE_TOGGLE_ATTRIBUTE}]`)
        ?.focus({ preventScroll: true });
  }, [open]);

  function keys(event: KeyboardEvent) {
    if (event.key !== "Escape" || event.defaultPrevented) return;
    event.preventDefault();
    setQueuePanelOpen(false);
  }

  return (
    <aside
      ref={panel}
      id={QUEUE_PANEL_ID}
      className="queue-dock"
      aria-label="Queue"
      data-open={open ? "" : undefined}
      inert={!open}
      onKeyDown={keys}
    >
      <div className="queue-panel">
        <QueueScreen docked onClose={() => setQueuePanelOpen(false)} />
      </div>
    </aside>
  );
}
