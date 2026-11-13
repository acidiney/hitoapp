import type { Schema } from "../../../shared/schema";
import { HttpError, requestJson } from "../../../shared/http";
import { log } from "../../../shared/log";

export type ArtworkKind = "recording" | "release" | "playlist";
type ArtworkTarget = { kind: ArtworkKind; id: string };
type ArtworkJob =
  | Schema<"ArtworkFetch">
  | Schema<"ReleaseArtworkFetch">
  | Schema<"PlaylistArtworkFetch">;

const routes = {
  recording: { collection: "recordings", receipts: "artwork-fetches" },
  release: { collection: "releases", receipts: "release-artwork-fetches" },
  playlist: { collection: "playlists", receipts: "playlist-artwork-fetches" },
} satisfies Record<ArtworkKind, { collection: string; receipts: string }>;

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

export type ArtworkSnapshot = {
  phase: Phase;
  revision: number;
  imageFailed: boolean;
};
export const idleArtwork: ArtworkSnapshot = {
  phase: "idle",
  revision: 0,
  imageFailed: false,
};

type Entry = {
  target: ArtworkTarget;
  key: string;
  controller: AbortController;
  snapshot: ArtworkSnapshot;
  running: boolean;
  imageStartedAt?: number;
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

let nextGeneration = 0;
const SETTLED_LIMIT = 4096;
const ADMISSION_BUSY = "artwork_admission_busy";
const ADMISSION_ATTEMPTS = 5;
const ADMISSION_WAIT_SECONDS = 2;
const ADMISSION_MAX_WAIT_SECONDS = 30;
const deferred = Symbol("admission deferred");

function admissionBusy(error: unknown): error is HttpError {
  return (
    error instanceof HttpError &&
    error.status === 429 &&
    error.code === ADMISSION_BUSY
  );
}

function sleepUntil(time: number, signal: AbortSignal) {
  return new Promise<void>((resolve, reject) => {
    const timer = setTimeout(
      () => {
        signal.removeEventListener("abort", cancel);
        resolve();
      },
      Math.max(0, time - Date.now()),
    );
    function cancel() {
      clearTimeout(timer);
      reject(signal.reason);
    }
    signal.addEventListener("abort", cancel, { once: true });
    if (signal.aborted) cancel();
  });
}

export class ArtworkRetrieval {
  readonly generation = ++nextGeneration;
  private entries = new Map<string, Entry>();
  private settled = new Map<string, ArtworkSnapshot>();
  private listeners = new Map<string, Set<() => void>>();
  private pending = new Set<Entry>();
  private waiting = new Set<() => void>();
  private requests = 0;
  private workflows = 0;
  private revision = 0;
  private active = true;
  private admissionUntil = 0;

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
    this.settled.clear();
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
    const key = keyFor(target);
    return (
      this.entries.get(key)?.snapshot ?? this.settled.get(key) ?? idleArtwork
    );
  }

  missing(target: ArtworkTarget, revision = 0) {
    const key = keyFor(target);
    if (!this.active) return;
    const entry = this.entries.get(key);
    if (entry) {
      this.observeImage(entry, revision, true);
    } else if (revision === 0 && !this.settled.has(key)) {
      this.start(target, { phase: "waiting", revision, imageFailed: true });
    }
  }

  loaded(target: ArtworkTarget, revision: number) {
    const entry = this.entries.get(keyFor(target));
    if (entry) this.observeImage(entry, revision, false);
  }

  private start(target: ArtworkTarget, snapshot: ArtworkSnapshot) {
    const key = keyFor(target);
    if (this.entries.size >= 128) {
      for (const candidate of this.entries.values()) {
        if (candidate.running || this.pending.has(candidate)) continue;
        this.entries.delete(candidate.key);
        this.remember(candidate);
        break;
      }
      if (this.entries.size >= 128) return;
    }
    const entry: Entry = {
      target,
      key,
      controller: new AbortController(),
      snapshot,
      running: false,
      imageStartedAt: snapshot.imageFailed ? undefined : performance.now(),
    };
    this.entries.set(key, entry);
    this.pending.add(entry);
    this.publish(entry, "waiting");
    this.drain();
  }

  private remember(entry: Entry) {
    const { phase } = entry.snapshot;
    if (phase !== "failed" && phase !== "unavailable") return;
    this.settled.delete(entry.key);
    this.settled.set(entry.key, entry.snapshot);
    if (this.settled.size > SETTLED_LIMIT)
      this.settled.delete(this.settled.keys().next().value!);
  }

  retry(target: ArtworkTarget) {
    const key = keyFor(target);
    this.settled.delete(key);
    const entry = this.entries.get(key);
    if (!this.active || (entry && (entry.running || this.pending.has(entry))))
      return;
    const revision = ++this.revision;
    if (entry?.snapshot.phase === "ready") {
      entry.imageStartedAt = performance.now();
      this.publish(entry, "ready", { failed: false, revision });
      return;
    }
    this.entries.delete(key);
    this.start(target, { phase: "waiting", revision, imageFailed: false });
  }

  refreshAvailability() {
    if (!this.active) return;
    const started = performance.now();
    let count = 0;
    for (const [key, snapshot] of [...this.settled]) {
      if (snapshot.phase !== "unavailable") continue;
      this.settled.delete(key);
      if (this.listeners.has(key)) {
        const [kind, id] = JSON.parse(key) as [ArtworkKind, string];
        this.missing({ kind, id });
      }
      count++;
    }
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

  private observeImage(entry: Entry, revision: number, failed: boolean) {
    if (!this.owns(entry) || entry.snapshot.revision !== revision) return;
    if (entry.imageStartedAt !== undefined) {
      log.debug("artwork.image_finished", {
        account_id: this.account,
        mode: entry.target.kind,
        ...(entry.target.kind === "recording"
          ? { recording_id: entry.target.id }
          : {}),
        ...(entry.target.kind === "playlist"
          ? { playlist_id: entry.target.id }
          : {}),
        reason: failed ? "load_failed" : "loaded",
        duration_ms: Math.round(performance.now() - entry.imageStartedAt),
      });
      entry.imageStartedAt = undefined;
    }
    this.publish(entry, entry.snapshot.phase, {
      failed: failed || entry.snapshot.imageFailed,
      revision,
    });
  }

  private publish(
    entry: Entry,
    phase: Phase,
    image?: { failed: boolean; revision: number },
  ) {
    if (!this.owns(entry)) return;
    if (phase === "ready" && !image) entry.imageStartedAt = performance.now();
    entry.snapshot = {
      phase,
      revision:
        image?.revision ??
        (phase === "ready" ? ++this.revision : entry.snapshot.revision),
      imageFailed:
        image?.failed ??
        (phase === "ready" ? false : entry.snapshot.imageFailed),
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
    admission = false,
  ): Promise<T> {
    const { signal } = entry.controller;
    await this.acquire(signal);
    try {
      if (!this.current(entry)) throw signal.reason;
      if (admission && Date.now() < this.admissionUntil) throw deferred;
      return await requestJson<T>(path, {
        action,
        method,
        account: this.account,
        signal,
      });
    } finally {
      this.requests--;
      this.waiting.values().next().value?.();
    }
  }

  private async admit(entry: Entry, path: string): Promise<ArtworkJob> {
    const { signal } = entry.controller;
    for (let rejected = 0; ; ) {
      while (Date.now() < this.admissionUntil)
        await sleepUntil(this.admissionUntil, signal);
      if (!this.current(entry)) throw signal.reason;
      try {
        return await this.read<ArtworkJob>(
          entry,
          path,
          "artwork.fetch",
          "POST",
          true,
        );
      } catch (error) {
        if (error === deferred) continue;
        if (!admissionBusy(error) || ++rejected >= ADMISSION_ATTEMPTS)
          throw error;
        const wait = Math.min(
          Math.max(error.retryAfter ?? ADMISSION_WAIT_SECONDS, 1),
          ADMISSION_MAX_WAIT_SECONDS,
        );
        this.admissionUntil = Math.max(
          this.admissionUntil,
          Date.now() + wait * 1000,
        );
        log.debug("artwork.admission_busy", {
          account_id: this.account,
          mode: entry.target.kind,
          count: rejected,
          duration_ms: wait * 1000,
        });
        this.publish(entry, "waiting");
      }
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
      ...(target.kind === "playlist" ? { playlist_id: target.id } : {}),
    };
    log.debug("artwork.started", fields);
    try {
      const { collection, receipts } = routes[target.kind];
      const base = `/${collection}/${encodeURIComponent(target.id)}/artwork`;
      const availability = await this.read<Schema<"ArtworkAvailability">>(
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
      job = await this.admit(entry, `${base}/fetch`);
      while (this.current(entry)) {
        const matches =
          target.kind === "recording"
            ? "recording_id" in job && job.recording_id === target.id
            : target.kind === "release"
              ? "release_id" in job && job.release_id === target.id
              : "playlist_id" in job && job.playlist_id === target.id;
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
