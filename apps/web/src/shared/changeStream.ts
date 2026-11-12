import { useEffect } from "react";
import type { Schema } from "./schema";
import { HttpError, requestJson } from "./http";
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

async function sessionStillActive(signal: AbortSignal) {
  try {
    await requestJson("/me", {
      action: "changes.session_check",
      cache: "no-store",
      signal,
    });
    return true;
  } catch (error) {
    if (error instanceof HttpError && [401, 403, 423].includes(error.status))
      return false;
    throw error;
  }
}

export type ChangeStreamOptions = {
  eventSource?: typeof EventSource;
  sessionActive?: (signal: AbortSignal) => Promise<boolean>;
  delays?: readonly number[];
};

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
  if (!Source) return () => {};
  const lifetime = new AbortController();
  let source: EventSource | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let failures = 0;
  let connectedOnce = false;

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
    if (lifetime.signal.aborted) return;
    if (!active) {
      log.info("changes.stream_stopped", { reason: "session_unavailable" });
      return;
    }
    const delay = delays[Math.min(failures, delays.length - 1)];
    failures += 1;
    log.debug("changes.stream_retry", { count: failures, duration_ms: delay });
    timer = setTimeout(() => {
      if (!lifetime.signal.aborted) connect();
    }, delay);
  }

  connect();
  return () => {
    lifetime.abort();
    clearTimeout(timer);
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
