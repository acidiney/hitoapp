import { useLayoutEffect, useRef, type RefObject } from "react";
import { nowPlayingOrigin } from "../nowPlayingOrigin";

const EXPAND_MS = 360;
const CONTENT_MS = 240;
const EASE = "cubic-bezier(0.2, 0, 0, 1)";

function reducedMotion(): boolean {
  return (
    window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false
  );
}

export function useNowPlayingMotion(
  room: RefObject<HTMLDialogElement | null>,
  cover: RefObject<HTMLElement | null>,
  finish: () => void,
) {
  const running = useRef<Animation[]>([]);
  const closing = useRef(false);
  useLayoutEffect(() => {
    const dialog = room.current;
    if (!dialog || reducedMotion() || !dialog.animate) return;
    const animations: Animation[] = [];
    const settled = getComputedStyle(dialog).backgroundColor;
    animations.push(
      dialog.animate(
        [{ backgroundColor: "rgba(0, 0, 0, 0)" }, { backgroundColor: settled }],
        { duration: EXPAND_MS, easing: EASE, fill: "both" },
      ),
    );
    for (const face of dialog.querySelectorAll("[data-np-fade]"))
      animations.push(
        face.animate([{ opacity: 0 }, { opacity: 1 }], {
          duration: CONTENT_MS,
          delay: EXPAND_MS - CONTENT_MS,
          easing: EASE,
          fill: "both",
        }),
      );
    const art = cover.current;
    const from = nowPlayingOrigin();
    if (art && from && from.width > 0) {
      const to = art.getBoundingClientRect();
      const scale = from.width / to.width;
      animations.push(
        art.animate(
          [
            {
              transformOrigin: "top left",
              transform: `translate(${from.left - to.left}px, ${from.top - to.top}px) scale(${scale})`,
              borderRadius: `${6 / scale}px`,
            },
            {
              transformOrigin: "top left",
              transform: "translate(0px, 0px) scale(1)",
              borderRadius: "16px",
            },
          ],
          { duration: EXPAND_MS, easing: EASE, fill: "both" },
        ),
      );
    }
    running.current = animations;
    return () => {
      for (const animation of animations) animation.cancel();
      running.current = [];
    };
  }, [room, cover]);
  function requestClose() {
    if (closing.current) return;
    closing.current = true;
    const animations = running.current;
    if (animations.length === 0) {
      finish();
      return;
    }
    for (const animation of animations) animation.reverse();
    void Promise.all(
      animations.map((animation) => animation.finished.catch(() => undefined)),
    ).then(finish);
  }
  return { requestClose };
}
