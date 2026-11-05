import { useEffect, useId, useRef, type ReactNode } from "react";

export default function Dialog({
  title,
  description,
  wide = false,
  busy = false,
  onClose,
  children,
}: {
  title: string;
  description?: string;
  wide?: boolean;
  busy?: boolean;
  onClose: () => void;
  children: ReactNode;
}) {
  const id = useId();
  const descriptionId = useId();
  const dialog = useRef<HTMLDialogElement>(null);
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(() => {
    const element = dialog.current;
    if (!element) return;
    const opener =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    const closed = () => close.current();
    element.addEventListener("close", closed);
    element.showModal();
    return () => {
      element.removeEventListener("close", closed);
      element.close();
      if (opener?.isConnected) opener.focus();
    };
  }, []);
  return (
    <dialog
      ref={dialog}
      className={wide ? "dialog dialog-wide" : "dialog"}
      aria-labelledby={id}
      aria-describedby={description ? descriptionId : undefined}
      onCancel={(event) => {
        event.preventDefault();
        if (!busy) close.current();
      }}
    >
      <h2 id={id}>{title}</h2>
      {description && (
        <p id={descriptionId} className="dialog-description">
          {description}
        </p>
      )}
      {children}
    </dialog>
  );
}
