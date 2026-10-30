import type { Schema } from "../../../shared/schema";

import { HttpError, request } from "../../../shared/http";
import { log } from "../../../shared/log";

type Scope = { signal: AbortSignal; active(): boolean };
type Pending = {
  source: string;
  claim: Schema<"SuccessfulPlayback">;
  scope: Scope;
};

export class BrowserCommunityReporting {
  private pending: Pending[] = [];
  private running = false;
  private closed = new AbortController();

  constructor(
    private account: string,
    private capture: () => Scope,
  ) {}

  activate() {
    if (this.closed.signal.aborted) this.closed = new AbortController();
  }

  admit(source: string, claim: Schema<"SuccessfulPlayback">) {
    const scope = this.capture();
    if (!scope.active() || this.closed.signal.aborted) return;
    if (this.pending.length >= 8) {
      log.debug("community_playback.dropped", {
        request_id: claim.submission_id,
        reason: "capacity",
      });
      return;
    }
    this.pending.push({ source, claim: Object.freeze({ ...claim }), scope });
    this.start();
  }

  close() {
    this.closed.abort();
    this.pending = [];
  }

  private start() {
    if (this.running || this.closed.signal.aborted) return;
    this.running = true;
    void this.drain().finally(() => {
      this.running = false;
      if (this.pending.length && !this.closed.signal.aborted) this.start();
    });
  }

  private async drain() {
    while (this.pending.length && !this.closed.signal.aborted) {
      const head = this.pending[0];
      const started = performance.now();
      let reason = "retired";
      for (
        let attempt = 0;
        attempt < 2 && head.scope.active() && !this.closed.signal.aborted;
        attempt++
      ) {
        const controller = new AbortController();
        const signal = AbortSignal.any([
          head.scope.signal,
          this.closed.signal,
          controller.signal,
        ]);
        const deadline = setTimeout(() => controller.abort(), 2000);
        try {
          reason = await this.send(head, signal);
          if (reason !== "retry") break;
        } catch {
          reason =
            signal.aborted && !controller.signal.aborted ? "retired" : "retry";
        } finally {
          clearTimeout(deadline);
        }
      }
      if (reason === "retry") reason = "exhausted";
      if (this.pending[0] === head) this.pending.shift();
      log.debug("community_playback.finished", {
        request_id: head.claim.submission_id,
        reason,
        duration_ms: Math.max(0, Math.round(performance.now() - started)),
      });
    }
  }

  private async send(
    head: Pending,
    signal: AbortSignal,
  ): Promise<Schema<"CommunityReportAdmission"> | "retry" | "rejected"> {
    const responsePromise = request(
      `/sources/${encodeURIComponent(head.source)}/successful-playback`,
      {
        action: "community.playback.report",
        errorBody: false,
        method: "POST",
        body: head.claim,
        account: this.account,
        signal,
      },
    );
    void responsePromise.then(
      (response) => {
        if (signal.aborted) void response.body?.cancel().catch(() => {});
      },
      () => {},
    );
    let response: Response;
    try {
      response = await abortable(responsePromise, signal);
    } catch (error) {
      if (!(error instanceof HttpError)) throw error;
      return error.status === 429 || error.status >= 500 ? "retry" : "rejected";
    }
    if (
      !response.headers.get("content-type")?.startsWith("application/json") ||
      !response.body
    ) {
      void response.body?.cancel().catch(() => {});
      return "rejected";
    }
    const reader = response.body.getReader();
    try {
      let body = "";
      let bytes = 0;
      const decoder = new TextDecoder("utf-8", { fatal: true });
      for (;;) {
        const chunk = await abortable(reader.read(), signal);
        if (chunk.done) break;
        bytes += chunk.value.byteLength;
        if (bytes > 1024) return "rejected";
        body += decoder.decode(chunk.value, { stream: true });
      }
      body += decoder.decode();
      const receipt: unknown = JSON.parse(body);
      return receipt === "queued" ||
        receipt === "ignored" ||
        receipt === "already_accepted"
        ? receipt
        : "rejected";
    } finally {
      void reader.cancel().catch(() => {});
      try {
        reader.releaseLock();
      } catch {}
    }
  }
}

function abortable<T>(work: Promise<T>, signal: AbortSignal): Promise<T> {
  if (signal.aborted)
    return Promise.reject(new DOMException("Retired", "AbortError"));
  return new Promise<T>((resolve, reject) => {
    const aborted = () => reject(new DOMException("Retired", "AbortError"));
    signal.addEventListener("abort", aborted, { once: true });
    work
      .then(resolve, reject)
      .finally(() => signal.removeEventListener("abort", aborted));
  });
}
