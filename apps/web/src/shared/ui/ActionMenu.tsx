import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { ChevronLeft, ChevronRight, MoreHorizontal } from "lucide-react";

export type ActionMenuItem = {
  label: string;
  onSelect?(): void;
  items?: ActionMenuItem[];
  destructive?: boolean;
  disabled?: boolean;
};

type MenuBox = { top: number; left: number; placement: "above" | "below" };

const gap = 6;
const edge = 8;

function reservedBottom(): number {
  const style = getComputedStyle(document.documentElement);
  return (
    (parseFloat(style.getPropertyValue("--player")) || 0) +
    (parseFloat(style.getPropertyValue("--tabs")) || 0)
  );
}

function placeMenu(
  trigger: DOMRect,
  menu: DOMRect,
  align: "start" | "end",
): MenuBox {
  const spaceBelow = window.innerHeight - reservedBottom() - trigger.bottom;
  const above =
    menu.height + gap > spaceBelow && trigger.top - edge >= menu.height;
  const top = above ? trigger.top - gap - menu.height : trigger.bottom + gap;
  const wanted = align === "end" ? trigger.right - menu.width : trigger.left;
  const left = Math.max(
    edge,
    Math.min(wanted, window.innerWidth - edge - menu.width),
  );
  return { top, left, placement: above ? "above" : "below" };
}

export default function ActionMenu({
  label,
  items,
  triggerClassName = "",
  icon,
  align = "start",
}: {
  label: string;
  items: ActionMenuItem[];
  triggerClassName?: string;
  icon?: ReactNode;
  align?: "start" | "end";
}) {
  const [open, setOpen] = useState(false);
  const [group, setGroup] = useState<ActionMenuItem | null>(null);
  const [box, setBox] = useState<MenuBox | null>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const focusPending = useRef(false);
  const menuId = useId();

  const enabled = () =>
    Array.from(
      list.current?.querySelectorAll<HTMLButtonElement>(
        '[role="menuitem"]:not(:disabled)',
      ) ?? [],
    );

  useLayoutEffect(() => {
    const element = list.current;
    const owner = trigger.current;
    if (!open || !element || !owner) return;
    setBox(
      placeMenu(
        owner.getBoundingClientRect(),
        element.getBoundingClientRect(),
        align,
      ),
    );
  }, [open, group, align]);

  useEffect(() => {
    if (!box || !focusPending.current) return;
    focusPending.current = false;
    enabled()[0]?.focus({ preventScroll: true });
  }, [box]);

  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!list.current?.contains(target) && !trigger.current?.contains(target))
        close(false);
    };
    const moved = () => {
      const owner = trigger.current;
      const element = list.current;
      if (owner && element)
        setBox(
          placeMenu(
            owner.getBoundingClientRect(),
            element.getBoundingClientRect(),
            align,
          ),
        );
    };
    document.addEventListener("pointerdown", outside);
    window.addEventListener("scroll", moved, true);
    window.addEventListener("resize", moved);
    return () => {
      document.removeEventListener("pointerdown", outside);
      window.removeEventListener("scroll", moved, true);
      window.removeEventListener("resize", moved);
    };
  }, [open, group, align]);

  function close(restoreFocus: boolean) {
    setOpen(false);
    setGroup(null);
    setBox(null);
    if (restoreFocus) trigger.current?.focus();
  }

  function show() {
    focusPending.current = true;
    setOpen(true);
  }

  function showGroup(item: ActionMenuItem | null) {
    focusPending.current = true;
    setGroup(item);
  }

  function choose(item: ActionMenuItem) {
    if (item.items) {
      showGroup(item);
      return;
    }
    close(true);
    item.onSelect?.();
  }

  function keys(event: React.KeyboardEvent) {
    const buttons = enabled();
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (event.key === "Escape") {
      event.preventDefault();
      close(true);
    } else if (event.key === "Tab") {
      close(true);
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      buttons[(at + step + buttons.length) % buttons.length]?.focus();
    } else if (event.key === "Home" || event.key === "End") {
      event.preventDefault();
      buttons[event.key === "Home" ? 0 : buttons.length - 1]?.focus();
    } else if (event.key === "ArrowLeft" && group) {
      event.preventDefault();
      showGroup(null);
    }
  }

  const shown = group?.items ?? items;

  return (
    <div className="action-menu">
      <button
        ref={trigger}
        type="button"
        className={`button action-menu-trigger ${triggerClassName}`.trim()}
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        onClick={() => (open ? close(false) : show())}
      >
        {icon ?? <MoreHorizontal size={20} aria-hidden="true" />}
      </button>
      {open &&
        createPortal(
          <div
            ref={list}
            className="action-menu-list"
            role="menu"
            id={menuId}
            aria-label={group ? group.label : label}
            data-placement={box?.placement ?? "below"}
            style={{
              position: "fixed",
              top: box?.top ?? 0,
              left: box?.left ?? 0,
              visibility: box ? "visible" : "hidden",
            }}
            onKeyDown={keys}
          >
            {group && (
              <button
                type="button"
                role="menuitem"
                className="action-menu-item action-menu-back"
                onClick={() => showGroup(null)}
              >
                <ChevronLeft size={16} aria-hidden="true" />
                {group.label}
              </button>
            )}
            {shown.map((item) => (
              <button
                key={item.label}
                type="button"
                role="menuitem"
                className="action-menu-item"
                data-tone={item.destructive ? "destructive" : undefined}
                aria-haspopup={item.items ? "menu" : undefined}
                disabled={item.disabled}
                onClick={() => choose(item)}
              >
                <span className="action-menu-label">{item.label}</span>
                {item.items && <ChevronRight size={16} aria-hidden="true" />}
              </button>
            ))}
          </div>,
          trigger.current?.closest("dialog") ?? document.body,
        )}
    </div>
  );
}
