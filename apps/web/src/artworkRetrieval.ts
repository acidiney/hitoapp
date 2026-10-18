import { request } from "./http";
import { log } from "./log";
import type { ArtworkAvailability, ArtworkFetch } from "./types";

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
  recording: string;
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

  subscribe(recording: string, listener: () => void) {
    const listeners = this.listeners.get(recording) ?? new Set();
    this.listeners.set(recording, listeners);
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
      if (listeners.size) return;
      this.listeners.delete(recording);
      const entry = this.entries.get(recording);
      if (!entry || (!entry.running && !this.pending.has(entry))) return;
      entry.controller.abort("cancelled");
      this.pending.delete(entry);
      this.entries.delete(recording);
    };
  }

  snapshot(recording: string) {
    return this.entries.get(recording)?.snapshot ?? idleArtwork;
  }

  missing(recording: string) {
    if (!this.active || this.entries.has(recording)) return;
    if (this.entries.size >= 128) {
      for (const candidate of this.entries.values()) {
        if (candidate.running || this.pending.has(candidate)) continue;
        this.entries.delete(candidate.recording);
        break;
      }
      if (this.entries.size >= 128) return;
    }
    const entry: Entry = {
      recording,
      controller: new AbortController(),
      snapshot: { phase: "waiting", revision: 0 },
      running: false,
    };
    this.entries.set(recording, entry);
    this.pending.add(entry);
    this.publish(entry, "waiting");
    this.drain();
  }

  retry(recording: string) {
    const entry = this.entries.get(recording);
    if (entry && (entry.running || this.pending.has(entry))) return;
    this.entries.delete(recording);
    this.missing(recording);
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
      this.entries.delete(entry.recording);
      if (this.listeners.has(entry.recording)) this.missing(entry.recording);
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
    return this.active && this.entries.get(entry.recording) === entry;
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
    for (const listener of this.listeners.get(entry.recording) ?? [])
      listener();
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
    let job: ArtworkFetch | undefined;
    const fields = { account_id: this.account, recording_id: entry.recording };
    log.debug("artwork.started", fields);
    try {
      const base = `/recordings/${encodeURIComponent(entry.recording)}/artwork`;
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
      job = await this.read<ArtworkFetch>(
        entry,
        `${base}/fetch`,
        "artwork.fetch",
        "POST",
      );
      while (this.current(entry)) {
        if (job.recording_id !== entry.recording)
          throw new Error("Artwork response did not match the recording");
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
        job = await this.read<ArtworkFetch>(
          entry,
          `/artwork-fetches/${encodeURIComponent(job.id)}`,
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
        this.entries.delete(entry.recording);
        this.entries.set(entry.recording, entry);
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
