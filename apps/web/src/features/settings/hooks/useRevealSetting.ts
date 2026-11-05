import { useEffect } from "react";

export type RevealRequest = { anchor: string; serial: number };

const attempts = 40;
const retryMs = 50;
const highlightMs = 1800;

function reveal(card: HTMLElement): number {
  const reduced = window.matchMedia?.(
    "(prefers-reduced-motion: reduce)",
  ).matches;
  card.scrollIntoView({
    block: "start",
    behavior: reduced ? "auto" : "smooth",
  });
  card.querySelector<HTMLElement>("h3")?.focus({ preventScroll: true });
  card.setAttribute("data-highlight", "true");
  return window.setTimeout(
    () => card.removeAttribute("data-highlight"),
    highlightMs,
  );
}

export function useRevealSetting(request: RevealRequest | null) {
  useEffect(() => {
    if (!request) return;
    let retry = 0;
    let fade = 0;
    let tries = 0;
    function look() {
      const card = document.getElementById(request!.anchor);
      if (card) {
        fade = reveal(card);
        return;
      }
      tries += 1;
      if (tries < attempts) retry = window.setTimeout(look, retryMs);
    }
    look();
    return () => {
      window.clearTimeout(retry);
      window.clearTimeout(fade);
      document
        .getElementById(request.anchor)
        ?.removeAttribute("data-highlight");
    };
  }, [request]);
}
