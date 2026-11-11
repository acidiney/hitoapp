import { useSyncExternalStore } from "react";
import { ConnectionError } from "../../../shared/connection";
import { HttpError, lockedEvent } from "../../../shared/http";
import { onInvalidated } from "../../../shared/invalidation";
import { log } from "../../../shared/log";
import type { Schema } from "../../../shared/schema";

export const FRESH_MS = 10 * 60_000;
export const STALE_MS = 6 * 60 * 60_000;
const MAX_ENTRIES = 200;
const RETRY_DELAY_MS = 700;
const MAX_RETRY_DELAY_MS = 5_000;
const TRANSIENT_STATUSES = [429, 502, 503, 504];

type Entry = { value: unknown; at: number; stale: boolean };
type Cacheable = { status: string };
type Flight = {
  promise: Promise<Cacheable>;
  controller: AbortController;
  waiters: number;
};

const entries = new Map<string, Entry>();
const groups = new Map<string, Schema<"CatalogReleaseGroup">>();
const refreshing = new Map<string, Promise<void>>();
const flights = new Map<string, Flight>();
const listeners = new Set<() => void>();
let busy = 0;
let clock = () => Date.now();
let retryDelayMs = RETRY_DELAY_MS;

function announce() {
  for (const listener of listeners) listener();
}

function remember(key: string, value: unknown) {
  entries.delete(key);
  entries.set(key, { value, at: clock(), stale: false });
  while (entries.size > MAX_ENTRIES) {
    const oldest = entries.keys().next().value;
    if (oldest === undefined) break;
    entries.delete(oldest);
  }
}

function lookup(key: string) {
  const hit = entries.get(key);
  if (hit) {
    entries.delete(key);
    entries.set(key, hit);
  }
  return hit;
}

function isFinal(value: Cacheable) {
  return value.status === "available";
}

function isTransient(error: unknown) {
  if (error instanceof ConnectionError) return true;
  return (
    error instanceof HttpError && TRANSIENT_STATUSES.includes(error.status)
  );
}

function backoff(error: unknown) {
  const hinted =
    error instanceof HttpError && error.retryAfter !== undefined
      ? error.retryAfter * 1000
      : 0;
  return Math.min(Math.max(hinted, retryDelayMs), MAX_RETRY_DELAY_MS);
}

function pause(ms: number, signal: AbortSignal) {
  return new Promise<void>((resolve, reject) => {
    const abort = () => {
      clearTimeout(timer);
      reject(signal.reason ?? new DOMException("Cancelled", "AbortError"));
    };
    const timer = setTimeout(() => {
      signal.removeEventListener("abort", abort);
      resolve();
    }, ms);
    if (signal.aborted) abort();
    else signal.addEventListener("abort", abort, { once: true });
  });
}

async function attempt<T extends Cacheable>(
  load: (signal: AbortSignal) => Promise<T>,
  signal: AbortSignal,
  retry: boolean,
): Promise<T> {
  let delay = retryDelayMs;
  try {
    return await load(signal);
  } catch (error) {
    if (!retry || signal.aborted || !isTransient(error)) throw error;
    delay = backoff(error);
  }
  log.debug("catalog_cache.retrying", { duration_ms: delay });
  await pause(delay, signal);
  return load(signal);
}

function join<T extends Cacheable>(
  key: string,
  load: (signal: AbortSignal) => Promise<T>,
  signal: AbortSignal,
  retry: boolean,
): Promise<T> {
  let flight = flights.get(key);
  if (!flight) {
    const controller = new AbortController();
    const created: Flight = {
      controller,
      waiters: 0,
      promise: attempt(load, controller.signal, retry)
        .then((value) => {
          if (isFinal(value)) remember(key, value);
          return value;
        })
        .finally(() => {
          if (flights.get(key) === created) flights.delete(key);
          controller.abort();
        }),
    };
    created.promise.catch(() => undefined);
    flights.set(key, created);
    flight = created;
  }
  const joined = flight;
  joined.waiters++;
  return new Promise<T>((resolve, reject) => {
    let done = false;
    const leave = () => {
      if (done) return;
      done = true;
      if (--joined.waiters === 0) {
        if (flights.get(key) === joined) flights.delete(key);
        joined.controller.abort();
      }
      reject(signal.reason ?? new DOMException("Cancelled", "AbortError"));
    };
    signal.addEventListener("abort", leave, { once: true });
    if (signal.aborted) leave();
    joined.promise.then(
      (value) => {
        if (done) return;
        done = true;
        signal.removeEventListener("abort", leave);
        resolve(value as T);
      },
      (error) => {
        if (done) return;
        done = true;
        signal.removeEventListener("abort", leave);
        reject(error);
      },
    );
  });
}

function refresh<T extends Cacheable>(
  key: string,
  load: (signal: AbortSignal) => Promise<T>,
  retry: boolean,
) {
  if (refreshing.has(key)) return;
  busy++;
  announce();
  const started = performance.now();
  const task = join(key, load, new AbortController().signal, retry)
    .then((value) => {
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
  retry = true,
): Promise<T> {
  const key = JSON.stringify([account, ...parts]);
  if (fresh) entries.delete(key);
  const hit = lookup(key);
  const age = hit ? clock() - hit.at : Infinity;
  if (hit && age < FRESH_MS && !hit.stale) return hit.value as T;
  if (hit && age < STALE_MS) {
    refresh(key, load, retry);
    return hit.value as T;
  }
  try {
    const value = await join(key, load, signal, retry);
    return isFinal(value) || !hit ? value : (hit.value as T);
  } catch (error) {
    if (hit && !signal.aborted) return hit.value as T;
    throw error;
  }
}

export function markCatalogStale() {
  for (const entry of entries.values()) entry.stale = true;
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
  if (account === undefined) {
    for (const flight of flights.values()) flight.controller.abort();
    flights.clear();
  }
  for (const key of [...entries.keys()])
    if (account === undefined || JSON.parse(key)[0] === account)
      entries.delete(key);
  for (const key of [...groups.keys()])
    if (account === undefined || JSON.parse(key)[0] === account)
      groups.delete(key);
}

onInvalidated("catalog", markCatalogStale);
onInvalidated("recording", (_id, change) => {
  if (change === undefined) markCatalogStale();
});
onInvalidated("resolved", markCatalogStale);
if (typeof window !== "undefined")
  window.addEventListener(lockedEvent, () => forgetCatalog());

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
  setRetryDelay(ms: number) {
    retryDelayMs = ms;
  },
  reset() {
    clock = () => Date.now();
    retryDelayMs = RETRY_DELAY_MS;
    entries.clear();
    flights.clear();
    refreshing.clear();
    groups.clear();
  },
};
