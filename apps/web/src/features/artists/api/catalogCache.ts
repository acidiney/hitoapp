import { useSyncExternalStore } from "react";
import { log } from "../../../shared/log";
import type { Schema } from "../../../shared/schema";

export const FRESH_MS = 10 * 60_000;
export const STALE_MS = 6 * 60 * 60_000;
const MAX_ENTRIES = 200;

type Entry = { value: unknown; at: number };
type Cacheable = { status: string };

const entries = new Map<string, Entry>();
const groups = new Map<string, Schema<"CatalogReleaseGroup">>();
const refreshing = new Map<string, Promise<void>>();
const listeners = new Set<() => void>();
let busy = 0;
let clock = () => Date.now();

function announce() {
  for (const listener of listeners) listener();
}

function remember(key: string, value: unknown) {
  entries.delete(key);
  entries.set(key, { value, at: clock() });
  while (entries.size > MAX_ENTRIES) {
    const oldest = entries.keys().next().value;
    if (oldest === undefined) break;
    entries.delete(oldest);
  }
}

function isFinal(value: Cacheable) {
  return value.status === "available";
}

function refresh<T extends Cacheable>(
  key: string,
  load: (signal: AbortSignal) => Promise<T>,
) {
  if (refreshing.has(key)) return;
  busy++;
  announce();
  const started = performance.now();
  const task = load(new AbortController().signal)
    .then((value) => {
      if (isFinal(value)) remember(key, value);
      log.debug("catalog_cache.refreshed", {
        reason: isFinal(value) ? "stored" : "kept_old",
        duration_ms: Math.round(performance.now() - started),
      });
    })
    .catch(() => {
      log.debug("catalog_cache.refreshed", { reason: "failed" });
    })
    .finally(() => {
      refreshing.delete(key);
      busy--;
      announce();
    });
  refreshing.set(key, task);
}

export async function cachedRead<T extends Cacheable>(
  account: string,
  parts: readonly unknown[],
  load: (signal: AbortSignal) => Promise<T>,
  signal: AbortSignal,
  fresh = false,
): Promise<T> {
  const key = JSON.stringify([account, ...parts]);
  if (fresh) entries.delete(key);
  const hit = entries.get(key);
  const age = hit ? clock() - hit.at : Infinity;
  if (hit && age < STALE_MS) {
    if (age >= FRESH_MS) refresh(key, load);
    return hit.value as T;
  }
  if (hit) entries.delete(key);
  const value = await load(signal);
  if (isFinal(value)) remember(key, value);
  return value;
}

export function forgetRead(account: string, parts: readonly unknown[]) {
  entries.delete(JSON.stringify([account, ...parts]));
}

const groupKey = (account: string, association: string, id: string) =>
  JSON.stringify([account, association, id]);

export function rememberReleaseGroups(
  account: string,
  association: string,
  found: readonly Schema<"CatalogReleaseGroup">[],
) {
  for (const group of found) {
    const key = groupKey(account, association, group.id);
    groups.delete(key);
    groups.set(key, group);
  }
  while (groups.size > MAX_ENTRIES * 20) {
    const oldest = groups.keys().next().value;
    if (oldest === undefined) break;
    groups.delete(oldest);
  }
}

export function rememberedReleaseGroup(
  account: string,
  association: string,
  id: string,
) {
  return groups.get(groupKey(account, association, id)) ?? null;
}

export function forgetCatalog(account?: string) {
  for (const key of [...entries.keys()])
    if (account === undefined || JSON.parse(key)[0] === account)
      entries.delete(key);
  for (const key of [...groups.keys()])
    if (account === undefined || JSON.parse(key)[0] === account)
      groups.delete(key);
}

export function useCatalogRefreshing() {
  return (
    useSyncExternalStore(
      (listener) => {
        listeners.add(listener);
        return () => listeners.delete(listener);
      },
      () => busy,
    ) > 0
  );
}

export const catalogCacheClock = {
  set(now: () => number) {
    clock = now;
  },
  reset() {
    clock = () => Date.now();
    entries.clear();
    groups.clear();
  },
};
