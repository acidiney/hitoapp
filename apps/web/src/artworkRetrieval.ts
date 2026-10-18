import { request } from "./http";
import { log } from "./log";
import type {
  ArtworkAvailability,
  ArtworkFetch,
  ReleaseArtworkFetch,
} from "./types";

export type ArtworkKind = "recording" | "release";
type ArtworkTarget = { kind: ArtworkKind; id: string };
type ArtworkJob = ArtworkFetch | ReleaseArtworkFetch;

function keyFor(target: ArtworkTarget) {
  return JSON.stringify([target.kind, target.id]);
}

type Phase =
  | "idle"
  | "waiting"
  | "checking"
  | "queued"
  | "running"
  | "ready"
  | "unavailable"
  | "failed"
  | "cancelled";

export type ArtworkSnapshot = { phase: Phase; revision: number };
export const idleArtwork: ArtworkSnapshot = { phase: "idle", revision: 0 };

type Entry = {
  target: ArtworkTarget;
  key: string;
  controller: AbortController;
  snapshot: ArtworkSnapshot;
  running: boolean;
};

function waitForPoll(signal: AbortSignal) {
  return new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => {
      signal.removeEventListener("abort", cancel);
      resolve();
    }, 1000);
    function cancel() {
      clearTimeout(timer);
      reject(signal.reason);
    }
    signal.addEventListener("abort", cancel, { once: true });
    if (signal.aborted) cancel();
  });
}

export class ArtworkRetrieval {
  private entries = new Map<string, Entry>();
  private listeners = new Map<string, Set<() => void>>();
  private pending = new Set<Entry>();
  private waiting = new Set<() => void>();
  private requests = 0;
  private workflows = 0;
  private revision = 0;
  private active = true;

  constructor(readonly account: string) {}

  open() {
    this.active = true;
  }

  close() {
    this.active = false;
    for (const entry of this.entries.values())
      entry.controller.abort("cancelled");
    this.pending.clear();
    this.entries.clear();
  }

  subscribe(target: ArtworkTarget, listener: () => void) {
    const key = keyFor(target);
    const listeners = this.listeners.get(key) ?? new Set();
    this.listeners.set(key, listeners);
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
      if (listeners.size) return;
      this.listeners.delete(key);
      const entry = this.entries.get(key);
      if (!entry || (!entry.running && !this.pending.has(entry))) return;
      entry.controller.abort("cancelled");
      this.pending.delete(entry);
      this.entries.delete(key);
    };
  }

  snapshot(target: ArtworkTarget) {
    return this.entries.get(keyFor(target))?.snapshot ?? idleArtwork;
  }

  missing(target: ArtworkTarget) {
    const key = keyFor(target);
    if (!this.active || this.entries.has(key)) return;
    if (this.entries.size >= 128) {
      for (const candidate of this.entries.values()) {
        if (candidate.running || this.pending.has(candidate)) continue;
        this.entries.delete(candidate.key);
        break;
      }
      if (this.entries.size >= 128) return;
    }
    const entry: Entry = {
      target,
      key,
      controller: new AbortController(),
      snapshot: { phase: "waiting", revision: 0 },
      running: false,
    };
    this.entries.set(key, entry);
    this.pending.add(entry);
    this.publish(entry, "waiting");
    this.drain();
  }

  retry(target: ArtworkTarget) {
    const key = keyFor(target);
    const entry = this.entries.get(key);
    if (entry && (entry.running || this.pending.has(entry))) return;
    this.entries.delete(key);
    this.missing(target);
  }

  refreshAvailability() {
    if (!this.active) return;
    const started = performance.now();
    let count = 0;
    for (const entry of [...this.entries.values()]) {
      if (
        entry.running ||
        this.pending.has(entry) ||
        entry.snapshot.phase !== "unavailable"
      )
        continue;
      this.entries.delete(entry.key);
      if (this.listeners.has(entry.key)) this.missing(entry.target);
      count++;
    }
    log.debug("artwork.availability_refreshed", {
      account_id: this.account,
      reason: "capability_approved",
      count,
      duration_ms: Math.round(performance.now() - started),
    });
  }

  private owns(entry: Entry) {
    return this.active && this.entries.get(entry.key) === entry;
  }

  private current(entry: Entry) {
    return this.owns(entry) && !entry.controller.signal.aborted;
  }

  private drain() {
    while (this.active && this.workflows < 16 && this.pending.size) {
      const entry = this.pending.values().next().value!;
      this.pending.delete(entry);
      if (!this.current(entry)) continue;
      entry.running = true;
      this.workflows++;
      this.publish(entry, "checking");
      void this.retrieve(entry);
    }
  }

  private publish(entry: Entry, phase: Phase) {
    if (!this.owns(entry)) return;
    entry.snapshot = {
      phase,
      revision: phase === "ready" ? ++this.revision : entry.snapshot.revision,
    };
    for (const listener of this.listeners.get(entry.key) ?? []) listener();
  }

  private async acquire(signal: AbortSignal) {
    if (signal.aborted) throw signal.reason;
    if (this.requests < 2) {
      this.requests++;
      return;
    }
    await new Promise<void>((resolve, reject) => {
      const resume = () => {
        this.waiting.delete(resume);
        signal.removeEventListener("abort", cancel);
        this.requests++;
        resolve();
      };
      const cancel = () => {
        this.waiting.delete(resume);
        reject(signal.reason);
      };
      this.waiting.add(resume);
      signal.addEventListener("abort", cancel, { once: true });
      if (signal.aborted) cancel();
    });
  }

  private async read<T>(
    entry: Entry,
    path: string,
    action: string,
    method: "GET" | "POST" = "GET",
  ): Promise<T> {
    const { signal } = entry.controller;
    await this.acquire(signal);
    try {
      if (!this.current(entry)) throw signal.reason;
      return await (
        await request(path, { action, method, account: this.account, signal })
      ).json();
    } finally {
      this.requests--;
      this.waiting.values().next().value?.();
    }
  }

  private async retrieve(entry: Entry) {
    const started = performance.now();
    const timer = setTimeout(() => entry.controller.abort("timeout"), 120_000);
    let phase: Phase = "failed";
    let reason = "failed";
    let job: ArtworkJob | undefined;
    const { target } = entry;
    const fields = {
      account_id: this.account,
      mode: target.kind,
      ...(target.kind === "recording" ? { recording_id: target.id } : {}),
    };
    log.debug("artwork.started", fields);
    try {
      const collection =
        target.kind === "recording" ? "recordings" : "releases";
      const receipts =
        target.kind === "recording"
          ? "artwork-fetches"
          : "release-artwork-fetches";
      const base = `/${collection}/${encodeURIComponent(target.id)}/artwork`;
      const availability = await this.read<ArtworkAvailability>(
        entry,
        `${base}/availability`,
        "artwork.availability",
      );
      if (!this.current(entry)) return;
      if (!availability.can_fetch) {
        phase = "unavailable";
        reason = "no_provider";
        return;
      }
      job = await this.read<ArtworkJob>(
        entry,
        `${base}/fetch`,
        "artwork.fetch",
        "POST",
      );
      while (this.current(entry)) {
        const matches =
          target.kind === "recording"
            ? "recording_id" in job && job.recording_id === target.id
            : "release_id" in job && job.release_id === target.id;
        if (!matches)
          throw new Error("Artwork response did not match its entity");
        if (
          job.status === "ready" ||
          job.status === "failed" ||
          job.status === "cancelled"
        ) {
          phase = job.status;
          reason = job.status;
          return;
        }
        this.publish(entry, job.status);
        await waitForPoll(entry.controller.signal);
        if (!this.current(entry)) return;
        job = await this.read<ArtworkJob>(
          entry,
          `/${receipts}/${encodeURIComponent(job.id)}`,
          "artwork.status",
        );
      }
    } catch {
      reason = "request_failed";
    } finally {
      clearTimeout(timer);
      this.workflows--;
      entry.running = false;
      if (entry.controller.signal.aborted) {
        reason =
          entry.controller.signal.reason === "timeout"
            ? "timeout"
            : "cancelled";
        phase = reason === "timeout" ? "failed" : "cancelled";
      }
      if (this.owns(entry)) {
        this.entries.delete(entry.key);
        this.entries.set(entry.key, entry);
        this.publish(entry, phase);
      }
      const record =
        phase === "ready"
          ? log.info
          : phase === "failed"
            ? log.warn
            : log.debug;
      record("artwork.finished", {
        ...fields,
        job_id: job?.id,
        reason,
        duration_ms: Math.round(performance.now() - started),
      });
      this.drain();
    }
  }
}
