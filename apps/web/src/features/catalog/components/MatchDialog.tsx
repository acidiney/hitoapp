import type { ReactNode } from "react";
import Dialog from "../../../shared/ui/Dialog";

export default function MatchDialog({
  title,
  busy = false,
  onClose,
  children,
}: {
  title: string;
  busy?: boolean;
  onClose: () => void;
  children: ReactNode;
}) {
  return (
    <Dialog
      title={title}
      wide
      busy={busy}
      className="match-dialog"
      onClose={onClose}
    >
      <div className="match-dialog-body" aria-busy={busy}>
        {children}
      </div>
    </Dialog>
  );
}
