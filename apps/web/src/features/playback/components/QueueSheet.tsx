import { useEffect, useRef } from "react";
import { setQueueSheetOpen, useQueueSheetOpen } from "../queueSheetState";
import QueueScreen from "./QueueScreen";

export default function QueueSheet() {
  const open = useQueueSheetOpen();
  if (!open) return null;
  return <OpenQueueSheet />;
}

function OpenQueueSheet() {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    element?.focus({ preventScroll: true });
    return () => element?.close();
  }, []);
  return (
    <dialog
      ref={dialog}
      className="queue-sheet"
      aria-label="Queue"
      tabIndex={-1}
      onClose={() => setQueueSheetOpen(false)}
      onClick={(event) => {
        if (event.target === event.currentTarget) setQueueSheetOpen(false);
      }}
    >
      <QueueScreen onClose={() => setQueueSheetOpen(false)} />
    </dialog>
  );
}
