import {
  useEffect,
  useId,
  useRef,
  type ReactNode,
  type RefObject,
} from "react";

export default function Dialog({
  title,
  eyebrow,
  lead,
  className = "",
  description,
  wide = false,
  busy = false,
  returnFocus,
  onClose,
  children,
}: {
  title: string;
  eyebrow?: string;
  lead?: ReactNode;
  className?: string;
  description?: string;
  wide?: boolean;
  busy?: boolean;
  returnFocus?: RefObject<HTMLElement | null>;
  onClose: () => void;
  children: ReactNode;
}) {
  const id = useId();
  const descriptionId = useId();
  const dialog = useRef<HTMLDialogElement>(null);
  const close = useRef(onClose);
  close.current = onClose;
  const returning = useRef(returnFocus);
  returning.current = returnFocus;
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
      const target = returning.current?.current ?? opener;
      if (target?.isConnected) target.focus();
    };
  }, []);
  return (
    <dialog
      ref={dialog}
      className={`${wide ? "dialog dialog-wide" : "dialog"} ${className}`.trim()}
      aria-labelledby={id}
      aria-describedby={description ? descriptionId : undefined}
      onCancel={(event) => {
        event.preventDefault();
        if (!busy) close.current();
      }}
    >
      {lead || eyebrow ? (
        <div className="dialog-head">
          {lead}
          <div>
            {eyebrow && <span className="dialog-eyebrow">{eyebrow}</span>}
            <h2 id={id}>{title}</h2>
          </div>
        </div>
      ) : (
        <h2 id={id}>{title}</h2>
      )}
      {description && (
        <p id={descriptionId} className="dialog-description">
          {description}
        </p>
      )}
      {children}
    </dialog>
  );
}
