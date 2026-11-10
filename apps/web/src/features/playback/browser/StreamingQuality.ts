import { useSyncExternalStore } from "react";
import type { Schema } from "../../../shared/schema";

export type StreamingQuality = Schema<"StreamQuality">;

const KEY = "hito:stream-quality";
const CHANGED = "hito:stream-quality-changed";
let sessionQuality: StreamingQuality | null = null;

export function streamingQuality(): StreamingQuality {
  if (sessionQuality !== null) return sessionQuality;
  try {
    const stored = localStorage.getItem(KEY);
    return stored === "high" || stored === "saver" ? stored : "original";
  } catch {
    return "original";
  }
}

export function subscribeStreamingQuality(notify: () => void) {
  const stored = (event: StorageEvent) => {
    if (event.key === KEY || event.key === null) {
      sessionQuality = null;
      notify();
    }
  };
  window.addEventListener(CHANGED, notify);
  window.addEventListener("storage", stored);
  return () => {
    window.removeEventListener(CHANGED, notify);
    window.removeEventListener("storage", stored);
  };
}

export function setStreamingQuality(quality: StreamingQuality) {
  try {
    localStorage.setItem(KEY, quality);
    sessionQuality = null;
  } catch {
    sessionQuality = quality;
  }
  window.dispatchEvent(new Event(CHANGED));
}

export function useStreamingQuality() {
  return useSyncExternalStore(subscribeStreamingQuality, streamingQuality);
}
