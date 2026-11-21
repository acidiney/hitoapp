import { useEffect } from "react";
import type { Schema } from "./schema";
import { lockedEvent } from "./http";
import { log } from "./log";
import { invalidate, type Invalidation } from "./invalidation";

export const changeStreamPath = "/api/v1/changes/events";

const reconnectDelaysMs = [1_000, 2_000, 5_000, 15_000, 30_000];

type ChangeNotice = Schema<"ChangeNotice">;

const everything: Invalidation[] = [
  { scope: "sources", id: null },
  { scope: "resolved", id: null },
  { scope: "recording", id: null },
  { scope: "playlist", id: null },
  { scope: "playlists", id: null },
];

export function noticeInvalidations(
  notice: ChangeNotice,
  firstConnection: boolean,
): Invalidation[] {
  switch (notice.kind) {
    case "resync":
      return notice.reason === "connected" && firstConnection ? [] : everything;
    case "sources":
      return [
        { scope: "sources", id: notice.recording_id },
        { scope: "resolved", id: notice.recording_id },
      ];
    case "recording":
      return [{ scope: "recording", id: notice.recording_id }];
    case "recordings":
      return [{ scope: "recording", id: null }];
    case "playlist":
      return [{ scope: "playlist", id: notice.playlist_id }];
    case "playlists":
      return [{ scope: "playlists", id: null }];
    case "artwork":
      return [{ scope: "artwork", id: `${notice.entity}:${notice.id}` }];
    default:
      return [];
  }
}

const sessionCheckTimeoutMs = 10_000;

export async function sessionStillActive(signal: AbortSignal) {
  const limit = new AbortController();
  const timer = setTimeout(() => limit.abort(), sessionCheckTimeoutMs);
  try {
    const response = await fetch("/api/v1/me", {
      credentials: "same-origin",
      cache: "no-store",
      headers: { "x-hito-request": "1" },
      signal: AbortSignal.any([signal, limit.signal]),
    });
    void response.body?.cancel().catch(() => {});
    if (response.status === 423) window.dispatchEvent(new Event(lockedEvent));
    if ([401, 403, 423].includes(response.status)) return false;
    if (!response.ok) throw new Error("Session check failed");
    return true;
  } finally {
    clearTimeout(timer);
  }
}

export type ChangeStreamOptions = {
  eventSource?: typeof EventSource;
  sessionActive?: (signal: AbortSignal) => Promise<boolean>;
  delays?: readonly number[];
  hiddenGraceMs?: number;
};

const hiddenGraceMs = 30_000;

function pageHidden() {
  return (
    typeof document !== "undefined" && document.visibilityState === "hidden"
  );
}

function parse(data: unknown): ChangeNotice | undefined {
  if (typeof data !== "string") return undefined;
  try {
    const value: unknown = JSON.parse(data);
    return value && typeof value === "object" && "kind" in value
      ? (value as ChangeNotice)
      : undefined;
  } catch {
    return undefined;
  }
}

export function openChangeStream(options: ChangeStreamOptions = {}) {
  const Source =
    "eventSource" in options ? options.eventSource : globalThis.EventSource;
  const sessionActive = options.sessionActive ?? sessionStillActive;
  const delays = options.delays ?? reconnectDelaysMs;
  const grace = options.hiddenGraceMs ?? hiddenGraceMs;
  if (!Source) return () => {};
  const lifetime = new AbortController();
  let source: EventSource | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let hiding: ReturnType<typeof setTimeout> | undefined;
  let failures = 0;
  let paused = pageHidden();
  let stopped = false;
  let connectedOnce = paused;

  function connect() {
    const next = new Source!(changeStreamPath, { withCredentials: true });
    source = next;
    next.onmessage = (event) => {
      const notice = parse(event.data);
      if (!notice) return;
      failures = 0;
      const first = notice.kind === "resync" && !connectedOnce;
      if (notice.kind === "resync" && notice.reason === "connected")
        connectedOnce = true;
      for (const change of noticeInvalidations(notice, first))
        invalidate(change.scope, change.id);
    };
    next.onerror = () => {
      next.close();
      if (source === next) source = undefined;
      void reconnect();
    };
  }

  async function reconnect() {
    let active: boolean;
    try {
      active = await sessionActive(lifetime.signal);
    } catch {
      active = !lifetime.signal.aborted;
    }
    if (lifetime.signal.aborted || paused) return;
    if (!active) {
      stopped = true;
      log.info("changes.stream_stopped", { reason: "session_unavailable" });
      return;
    }
    const delay = delays[Math.min(failures, delays.length - 1)];
    failures += 1;
    log.debug("changes.stream_retry", { count: failures, duration_ms: delay });
    timer = setTimeout(() => {
      timer = undefined;
      if (!lifetime.signal.aborted && !paused) connect();
    }, delay);
  }

  function pause() {
    hiding = undefined;
    paused = true;
    connectedOnce = true;
    clearTimeout(timer);
    timer = undefined;
    source?.close();
    source = undefined;
    log.debug("changes.stream_paused", { reason: "page_hidden" });
  }

  function visibilityChanged() {
    if (pageHidden()) {
      if (!paused && hiding === undefined) hiding = setTimeout(pause, grace);
      return;
    }
    clearTimeout(hiding);
    hiding = undefined;
    if (!paused) return;
    paused = false;
    if (!stopped && !source && timer === undefined) connect();
  }

  if (!paused) connect();
  document.addEventListener("visibilitychange", visibilityChanged);
  return () => {
    lifetime.abort();
    document.removeEventListener("visibilitychange", visibilityChanged);
    clearTimeout(timer);
    clearTimeout(hiding);
    source?.close();
    source = undefined;
  };
}

export function useChangeStream(account: string, enabled: boolean) {
  useEffect(
    () => (enabled ? openChangeStream() : undefined),
    [account, enabled],
  );
}
