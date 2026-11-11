import type { RefObject } from "react";

import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";

export default function PluginConfirmDialog({
  title,
  consequence,
  confirmLabel,
  pendingLabel,
  error,
  pending,
  returnFocus,
  onConfirm,
  onCancel,
}: {
  title: string;
  consequence: string;
  confirmLabel: string;
  pendingLabel: string;
  error: string;
  pending: boolean;
  returnFocus?: RefObject<HTMLElement | null>;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <Dialog
      title={title}
      busy={pending}
      returnFocus={returnFocus}
      onClose={onCancel}
    >
      <p className="muted">{consequence}</p>
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      <div className="dialog-actions">
        <Button disabled={pending} onClick={onCancel}>
          Cancel
        </Button>
        <Button variant="destructive" disabled={pending} onClick={onConfirm}>
          {pending ? pendingLabel : confirmLabel}
        </Button>
      </div>
    </Dialog>
  );
}
