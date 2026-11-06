import type { Schema } from "../../../shared/schema";

const VERSION = 1;
const PREFIX = "hito:queue:";
export const STORED_QUEUE_LIMIT = 500;
export const STORED_QUEUE_LOOKBEHIND = 50;

export type StoredEntry = {
  id: string;
  chapter?: Schema<"AudiobookChapterAnchor">;
};

export type StoredQueue = {
  entries: StoredEntry[];
  index: number;
  position: number;
};

export function queueStorageKey(account: string) {
  return `${PREFIX}v${VERSION}:${account}`;
}

export function browserStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

function text(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= 200;
}

function chapter(
  value: unknown,
  recording: string,
): Schema<"AudiobookChapterAnchor"> | null {
  if (typeof value !== "object" || value === null) return null;
  const item = value as Record<string, unknown>;
  if (
    !text(item.audiobook_id) ||
    !text(item.entry_id) ||
    item.recording_id !== recording
  )
    return null;
  return {
    audiobook_id: item.audiobook_id,
    entry_id: item.entry_id,
    recording_id: item.recording_id,
  };
}

export function parseStoredQueue(raw: string | null): StoredQueue | null {
  if (!raw) return null;
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return null;
  }
  if (typeof value !== "object" || value === null) return null;
  const wire = value as Record<string, unknown>;
  if (
    wire.version !== VERSION ||
    !Array.isArray(wire.entries) ||
    !Number.isInteger(wire.index) ||
    typeof wire.position !== "number" ||
    !Number.isFinite(wire.position)
  )
    return null;
  const entries: StoredEntry[] = [];
  let index = -1;
  wire.entries.forEach((entry, at) => {
    const id = (entry as { id?: unknown } | null)?.id;
    if (!text(id)) return;
    const anchor = (entry as { chapter?: unknown }).chapter;
    const anchored = anchor === undefined ? null : chapter(anchor, id);
    if (anchor !== undefined && !anchored) return;
    if (at === wire.index) index = entries.length;
    entries.push(anchored ? { id, chapter: anchored } : { id });
  });
  if (index < 0) return null;
  return { entries, index, position: Math.max(0, wire.position as number) };
}

export function serializeQueue(
  entries: StoredEntry[],
  index: number,
  position: number,
): string {
  return JSON.stringify({
    version: VERSION,
    index,
    position: Math.round(position),
    entries,
  });
}

export function readStoredQueue(
  storage: Storage | null,
  account: string,
): StoredQueue | null {
  try {
    return parseStoredQueue(storage?.getItem(queueStorageKey(account)) ?? null);
  } catch {
    return null;
  }
}

export function writeStoredQueue(
  storage: Storage | null,
  account: string,
  value: string,
) {
  try {
    storage?.setItem(queueStorageKey(account), value);
  } catch {
    return;
  }
}

export function removeStoredQueue(storage: Storage | null, account: string) {
  try {
    storage?.removeItem(queueStorageKey(account));
  } catch {
    return;
  }
}

export function clearStoredQueues(
  storage: Storage | null = browserStorage(),
  except?: string,
) {
  try {
    if (!storage) return;
    const keep = except === undefined ? null : queueStorageKey(except);
    const keys: string[] = [];
    for (let at = 0; at < storage.length; at++) {
      const key = storage.key(at);
      if (key?.startsWith(PREFIX) && key !== keep) keys.push(key);
    }
    keys.forEach((key) => storage.removeItem(key));
  } catch {
    return;
  }
}
