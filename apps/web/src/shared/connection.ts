import { log } from "./log";

export type OutageReason = "network" | "timeout" | "gateway";
export type ConnectionState =
  | { reachable: true }
  | { reachable: false; reason: OutageReason };

export const connectionRestoredEvent = "hito-connection-restored";

const probeDelaysMs = [2_000, 4_000, 8_000, 15_000];
const probeTimeoutMs = 5_000;

const messages: Record<OutageReason, string> = {
  network: "Hito cannot be reached right now. Try again in a moment.",
  timeout: "Hito is taking too long to answer. Try again in a moment.",
  gateway: "Hito is not responding right now. Try again in a moment.",
};

export class ConnectionError extends TypeError {
  constructor(readonly reason: OutageReason) {
    super(messages[reason]);
    this.name = "ConnectionError";
  }
}

export const gatewayMessage = messages.gateway;

const reachableState: ConnectionState = { reachable: true };
let state: ConnectionState = reachableState;
let lostAt = 0;
let probes = 0;
let probing = false;
let lostChange = false;
let timer: ReturnType<typeof setTimeout> | undefined;
const listeners = new Set<() => void>();

function publish(next: ConnectionState) {
  state = next;
  for (const listener of [...listeners]) listener();
}

export function getConnection() {
  return state;
}

export function subscribeConnection(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function schedule() {
  clearTimeout(timer);
  const delay = probeDelaysMs[Math.min(probes, probeDelaysMs.length - 1)];
  timer = setTimeout(() => void probe(), delay);
}

async function probe() {
  if (state.reachable || probing) return;
  probing = true;
  probes += 1;
  const attempt = new AbortController();
  const limit = setTimeout(() => attempt.abort(), probeTimeoutMs);
  let up = false;
  try {
    const response = await fetch("/health", {
      cache: "no-store",
      credentials: "omit",
      signal: attempt.signal,
    });
    up = response.ok;
    void response.body?.cancel().catch(() => {});
  } catch {
    up = false;
  } finally {
    clearTimeout(limit);
    probing = false;
  }
  log.debug("connection.probe", { count: probes, enabled: up });
  if (up) reportReachable();
  else if (!state.reachable) schedule();
}

export function reportUnreachable(reason: OutageReason) {
  if (!state.reachable) return;
  lostAt = Date.now();
  probes = 0;
  log.warn("connection.lost", { reason });
  publish({ reachable: false, reason });
  schedule();
}

export function reportReachable() {
  if (state.reachable) return;
  clearTimeout(timer);
  log.info("connection.restored", {
    count: probes,
    duration_ms: Date.now() - lostAt,
  });
  publish(reachableState);
  window.dispatchEvent(new Event(connectionRestoredEvent));
}

export function retryConnectionNow() {
  if (state.reachable) return;
  clearTimeout(timer);
  void probe();
}

export function watchServer(onRestored: () => void) {
  window.addEventListener(connectionRestoredEvent, onRestored);
  reportUnreachable("network");
  return () => window.removeEventListener(connectionRestoredEvent, onRestored);
}

export function getLostChange() {
  return lostChange;
}

export function markChangeLost() {
  if (lostChange) return;
  lostChange = true;
  log.warn("connection.change_lost", { reason: "not_saved" });
  for (const listener of [...listeners]) listener();
}

export function dismissLostChange() {
  if (!lostChange) return;
  lostChange = false;
  for (const listener of [...listeners]) listener();
}

export function clearOutage() {
  clearTimeout(timer);
  timer = undefined;
  probing = false;
  probes = 0;
  lostChange = false;
  state = reachableState;
  for (const listener of [...listeners]) listener();
}

export function resetConnection() {
  clearOutage();
  listeners.clear();
}

if (typeof window !== "undefined") {
  window.addEventListener("online", retryConnectionNow);
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") retryConnectionNow();
  });
}
