import { useRef, useState, type ReactNode } from "react";
import { ChevronLeft, ChevronRight } from "lucide-react";

import { useScrollFade, type ScrollEdges } from "../hooks/useScrollFade";

const STEP = 0.8;

function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

export default function ShelfRow({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  const row = useRef<HTMLUListElement>(null);
  const [edges, setEdges] = useState<ScrollEdges>({ start: false, end: false });
  useScrollFade(row, setEdges);
  function scroll(direction: 1 | -1) {
    const element = row.current;
    if (!element) return;
    element.scrollBy({
      left: direction * Math.round(element.clientWidth * STEP),
      behavior: prefersReducedMotion() ? "auto" : "smooth",
    });
  }
  return (
    <div className="shelf">
      <ul className="shelf-row" ref={row}>
        {children}
      </ul>
      {edges.start && (
        <button
          type="button"
          className="shelf-nav"
          data-side="start"
          aria-label={`Previous in ${label}`}
          onClick={() => scroll(-1)}
        >
          <ChevronLeft size={20} aria-hidden="true" />
        </button>
      )}
      {edges.end && (
        <button
          type="button"
          className="shelf-nav"
          data-side="end"
          aria-label={`Next in ${label}`}
          onClick={() => scroll(1)}
        >
          <ChevronRight size={20} aria-hidden="true" />
        </button>
      )}
    </div>
  );
}
