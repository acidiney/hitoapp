import { useCallback, useEffect, useRef, useState } from "react";
import {
  editableTarget,
  readTrackLocation,
  trackAnchors,
  trackTabForShortcut,
  trackTabHref,
  type TrackAnchor,
  type TrackTab,
} from "../model/trackTabs";

export function useTrackTab(recording: string) {
  const initial = useRef(
    readTrackLocation(window.location.search, window.location.hash),
  );
  const [tab, setTab] = useState<TrackTab>(initial.current.tab);
  const [request, setRequest] = useState<{
    anchor: TrackAnchor;
    serial: number;
  } | null>(
    initial.current.anchor
      ? { anchor: initial.current.anchor, serial: 0 }
      : null,
  );

  const select = useCallback(
    (next: TrackTab) => {
      setTab(next);
      window.history.replaceState(
        window.history.state,
        "",
        trackTabHref(recording, next),
      );
    },
    [recording],
  );

  const reveal = useCallback((anchor: TrackAnchor) => {
    setTab(trackAnchors[anchor]);
    setRequest((before) => ({ anchor, serial: (before?.serial ?? 0) + 1 }));
  }, []);

  useEffect(() => {
    if (!request) return;
    const heading = document.getElementById(request.anchor);
    if (!heading || heading.closest('[data-active="false"]')) return;
    heading.scrollIntoView?.({ block: "start" });
    heading.focus({ preventScroll: true });
  }, [request]);

  useEffect(() => {
    function shortcut(event: KeyboardEvent) {
      if (event.ctrlKey || event.metaKey || event.altKey) return;
      if (editableTarget(event.target)) return;
      const next = trackTabForShortcut(event.key);
      if (next) select(next);
    }
    document.addEventListener("keydown", shortcut);
    return () => document.removeEventListener("keydown", shortcut);
  }, [select]);

  return { tab, select, reveal };
}
