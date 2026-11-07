import { useEffect, type RefObject } from "react";

export function useScrollFade(ref: RefObject<HTMLElement | null>) {
  useEffect(() => {
    const strip = ref.current;
    if (!strip) return;
    function update() {
      if (!strip) return;
      const hidden = strip.scrollWidth - strip.clientWidth;
      strip.toggleAttribute(
        "data-fade-start",
        hidden > 1 && strip.scrollLeft > 1,
      );
      strip.toggleAttribute(
        "data-fade-end",
        hidden > 1 && strip.scrollLeft < hidden - 1,
      );
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
