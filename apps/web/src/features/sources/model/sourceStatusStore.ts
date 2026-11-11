import type { Schema } from "../../../shared/schema";
import { sourcesApi } from "../api/sourcesApi";

export type SourceStatus = {
  loading: boolean;
  source: Schema<"Source"> | null;
};

type Entry = {
  status: SourceStatus;
  controller: AbortController | null;
  settled: boolean;
};

const LOADING: SourceStatus = { loading: true, source: null };
const entries = new Map<string, Entry>();
const listeners = new Set<() => void>();
const keyOf = (account: string, recording: string) =>
  `${account}\u0000${recording}`;
const notify = () => listeners.forEach((listener) => listener());

function bestSource(sources: unknown): Schema<"Source"> | null {
  if (!Array.isArray(sources)) return null;
  const list = sources as Schema<"Source">[];
  return list.find((item) => item.ready) ?? list[0] ?? null;
}

function sameSource(a: Schema<"Source"> | null, b: Schema<"Source"> | null) {
  if (a === b) return true;
  if (!a || !b) return false;
  return (
    a.id === b.id &&
    a.ready === b.ready &&
    a.provider === b.provider &&
    a.label === b.label &&
    a.match_kind === b.match_kind
  );
}

export const sourceStatusStore = {
  subscribe(listener: () => void) {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  },
  peek(account: string, recording: string): SourceStatus {
    return entries.get(keyOf(account, recording))?.status ?? LOADING;
  },
  known(account: string, recording: string) {
    return entries.get(keyOf(account, recording))?.settled === true;
  },
  refresh(account: string, recording: string) {
    const key = keyOf(account, recording);
    const existing = entries.get(key);
    if (existing?.controller) return;
    const controller = new AbortController();
    const entry: Entry = existing ?? {
      status: LOADING,
      controller: null,
      settled: false,
    };
    entry.controller = controller;
    entries.set(key, entry);
    const settle = (source: Schema<"Source"> | null) => {
      if (controller.signal.aborted || entries.get(key) !== entry) return;
      entry.controller = null;
      entry.settled = true;
      if (entry.status.loading || !sameSource(entry.status.source, source)) {
        entry.status = { loading: false, source };
        notify();
      }
    };
    sourcesApi
      .playbackSources(account, recording, controller.signal)
      .then((sources) => settle(bestSource(sources)))
      .catch(() => settle(null));
  },
  invalidate(account: string, recording: string) {
    const key = keyOf(account, recording);
    const entry = entries.get(key);
    if (!entry) return;
    entry.controller?.abort();
    entry.controller = null;
    this.refresh(account, recording);
  },
  reset() {
    entries.forEach((entry) => entry.controller?.abort());
    entries.clear();
  },
};
