import {
  useEffect,
  useId,
  useRef,
  type ReactNode,
  type RefObject,
} from "react";

import { followKeyboard, lockPageScroll } from "./sheetViewport";

const LEAVE_MS = 180;

function leavingCopy(element: HTMLDialogElement) {
  if (
    !element.open ||
    typeof window.matchMedia !== "function" ||
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  )
    return null;
  const ghost = element.cloneNode(true) as HTMLDialogElement;
  ghost.removeAttribute("open");
  ghost.removeAttribute("id");
  ghost.setAttribute("data-leaving", "");
  ghost.setAttribute("aria-hidden", "true");
  return {
    show() {
      if (typeof ghost.showPopover !== "function") return;
      ghost.setAttribute("popover", "manual");
      ghost.setAttribute("open", "");
      ghost.setAttribute("inert", "");
      document.body.append(ghost);
      ghost.showPopover();
      window.setTimeout(() => {
        ghost.remove();
      }, LEAVE_MS);
    },
  };
}

function settleFocus(element: HTMLDialogElement) {
  const focused = document.activeElement;
  const heading = element.querySelector<HTMLElement>("h2");
  if (
    heading &&
    focused instanceof HTMLElement &&
    element.contains(focused) &&
    focused.matches("button, a[href]")
  ) {
    heading.tabIndex = -1;
    heading.focus({ preventScroll: true });
  }
  element.scrollTop = 0;
}

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
    settleFocus(element);
    const unlock = lockPageScroll();
    const unfollow = followKeyboard(element);
    return () => {
      unlock();
      unfollow();
      element.removeEventListener("close", closed);
      const ghost = leavingCopy(element);
      element.close();
      ghost?.show();
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
