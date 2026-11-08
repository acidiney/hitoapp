import { useEffect, useRef, type RefObject } from "react";

export type ScrollEdges = { start: boolean; end: boolean };

export function useScrollFade(
  ref: RefObject<HTMLElement | null>,
  onChange?: (edges: ScrollEdges) => void,
) {
  const latest = useRef(onChange);
  latest.current = onChange;
  useEffect(() => {
    const strip = ref.current;
    if (!strip) return;
    function update() {
      if (!strip) return;
      const hidden = strip.scrollWidth - strip.clientWidth;
      const start = hidden > 1 && strip.scrollLeft > 1;
      const end = hidden > 1 && strip.scrollLeft < hidden - 1;
      strip.toggleAttribute("data-fade-start", start);
      strip.toggleAttribute("data-fade-end", end);
      latest.current?.({ start, end });
    }
    update();
    strip.addEventListener("scroll", update, { passive: true });
    const observer =
      typeof ResizeObserver === "undefined" ? null : new ResizeObserver(update);
    observer?.observe(strip);
    return () => {
      strip.removeEventListener("scroll", update);
      observer?.disconnect();
    };
  }, [ref]);
}
