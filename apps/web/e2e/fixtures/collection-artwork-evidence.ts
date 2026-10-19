import { writeFile } from "node:fs/promises";
import { expect, type BrowserContext, type Page } from "@playwright/test";
import { captureConsolePrivacy } from "./console-privacy";
import type { Entity } from "./collection-artwork-provider";

export type Receipt = {
  kind: Entity;
  target: string;
  id: string;
  status: string;
  attempts: number;
};
const collections = {
  recordings: "recording",
  releases: "release",
  playlists: "playlist",
} as const;

export function collectionArtworkEvidence(origin: string) {
  const logs: string[] = [];
  const pending = new Set<Promise<void>>();
  const failures: string[] = [];
  const requests: { path: string; method: string; type: string }[] = [];
  const responses: {
    path: string;
    status: number;
    request: string | undefined;
  }[] = [];
  const enqueues: Receipt[] = [];
  const pageErrors: { name: string; document: string; code: string }[] = [];
  let phase = "setup";
  let crossOrigin = 0;
  const http: {
    request_id: string;
    action: string;
    duration_ms: number;
    status: number;
  }[] = [];
  const artwork: Record<string, unknown>[] = [];
  const sessionCanaries = new Set<string>();
  const storageSnapshots: string[] = [];

  async function captureSession(context: BrowserContext) {
    for (const cookie of await context.cookies()) {
      if (cookie.value) sessionCanaries.add(cookie.value);
    }
    for (const page of context.pages()) {
      if (new URL(page.url()).origin !== origin) continue;
      storageSnapshots.push(
        await page.evaluate(() =>
          JSON.stringify({
            local: Object.entries(localStorage),
            session: Object.entries(sessionStorage),
          }),
        ),
      );
    }
  }

  async function observe(context: BrowserContext) {
    await captureConsolePrivacy(
      context,
      (snapshot, capture) => {
        if (logs.length >= 20_000) {
          failures.push("console_capacity");
          return;
        }
        logs.push(snapshot);
        if (capture !== "snapshot") return;
        const values = JSON.parse(snapshot) as unknown[];
        if (
          values[0] !== "[hito]" ||
          typeof values[1] !== "object" ||
          values[1] === null
        )
          return;
        const entry = values[1] as Record<string, unknown>;
        const allowed = [
          "event",
          "request_id",
          "account_id",
          "recording_id",
          "source_id",
          "playlist_id",
          "job_id",
          "library_id",
          "folder_id",
          "operation_id",
          "preparation_id",
          "method",
          "route",
          "action",
          "page",
          "reason",
          "mode",
          "status",
          "content_revision",
          "duration_ms",
          "count",
          "index",
          "position_ms",
          "speed",
          "volume",
          "enabled",
        ];
        if (Object.keys(entry).some((key) => !allowed.includes(key)))
          failures.push("unexpected_log_field");
        if (
          entry.event === "http.response" &&
          typeof entry.action === "string" &&
          entry.action.startsWith("artwork.")
        ) {
          if (
            typeof entry.request_id !== "string" ||
            typeof entry.duration_ms !== "number" ||
            entry.duration_ms < 0 ||
            typeof entry.status !== "number"
          )
            failures.push("uncorrelated_http");
          else http.push(entry as (typeof http)[number]);
        }
        if (
          typeof entry.event === "string" &&
          entry.event.startsWith("artwork.")
        )
          artwork.push(entry);
      },
      (failure) => failures.push(failure),
      pending,
    );
    await context.addInitScript((expectedOrigin) => {
      if (location.origin === expectedOrigin)
        localStorage.setItem("hito-log-level", "debug");
    }, origin);
    const observePage = (page: Page) =>
      page.on("pageerror", (error) =>
        pageErrors.push({
          name: error.name,
          document: page.url() === "about:blank" ? "blank" : "application",
          code:
            error.name === "SecurityError" &&
            error.message.includes("localStorage") &&
            error.message.includes("Access is denied")
              ? "local_storage_denied"
              : "unclassified",
        }),
      );
    context.on("page", observePage);
    for (const page of context.pages()) observePage(page);
    context.on("request", (request) => {
      const url = new URL(request.url());
      if (url.origin !== origin) crossOrigin++;
      if (requests.length >= 800) {
        failures.push("request_capacity");
        return;
      }
      requests.push({
        path: url.pathname,
        method: request.method(),
        type: request.resourceType(),
      });
    });
    context.on("response", (response) => {
      const url = new URL(response.url());
      if (url.origin !== origin) return;
      responses.push({
        path: url.pathname,
        status: response.status(),
        request: response.headers()["x-request-id"],
      });
      const match =
        /^\/api\/v1\/(recordings|releases|playlists)\/([^/]+)\/artwork\/fetch$/.exec(
          url.pathname,
        );
      if (!match || response.request().method() !== "POST") return;
      const reading = (async () => {
        expect(response.status()).toBe(202);
        expect(response.request().postData()).toBeNull();
        const bytes = await response.body();
        expect(bytes.length).toBeLessThanOrEqual(8192);
        const receipt = JSON.parse(bytes.toString()) as Record<string, unknown>;
        const kind = collections[match[1] as keyof typeof collections];
        expect(receipt[`${kind === "release" ? "release" : kind}_id`]).toBe(
          match[2],
        );
        expect(typeof receipt.id).toBe("string");
        expect(["queued", "running", "ready"]).toContain(receipt.status);
        enqueues.push({
          kind,
          target: match[2],
          id: receipt.id as string,
          status: receipt.status as string,
          attempts: receipt.attempts as number,
        });
      })()
        .catch(() => {
          failures.push(`invalid_${phase}_receipt`);
        })
        .finally(() => pending.delete(reading));
      pending.add(reading);
    });
  }

  async function settle() {
    while (pending.size) await Promise.all([...pending]);
  }

  async function verify(privateValues: string[], ready: Receipt[]) {
    await settle();
    await writeFile(
      "/evidence/browser-errors.json",
      JSON.stringify(pageErrors),
      { mode: 0o600 },
    );
    expect(failures).toEqual([]);
    expect(pageErrors).toEqual([]);
    expect(crossOrigin).toBe(0);
    expect(
      requests.filter(
        (request) =>
          request.type === "media" || /\/audio(?:\/|$)/.test(request.path),
      ),
    ).toEqual([]);
    expect(
      requests.filter(
        (request) =>
          request.method !== "GET" &&
          (/\/(?:downloads?|sources|source-choice)(?:\/|$)/.test(
            request.path,
          ) ||
            request.path === "/api/v1/sync"),
      ),
    ).toEqual([]);
    expect(requests.length).toBeLessThan(800);
    const text = logs.join("\n");
    const canaries = [...new Set([...privateValues, ...sessionCanaries])];
    expect(sessionCanaries.size).toBeGreaterThanOrEqual(4);
    for (const value of canaries) {
      for (const pattern of [
        value,
        JSON.stringify(value).slice(1, -1),
        encodeURIComponent(value),
      ])
        expect(text.includes(pattern)).toBe(false);
      for (const storage of storageSnapshots)
        expect(storage.includes(value)).toBe(false);
    }
    await writeFile(
      "/evidence/artwork-job-observations.json",
      JSON.stringify({
        receipts: enqueues,
        finished: artwork
          .filter((entry) => entry.event === "artwork.finished")
          .map((entry) => ({
            job: entry.job_id,
            mode: entry.mode,
            reason: entry.reason,
            duration_ms: entry.duration_ms,
          })),
      }),
      { mode: 0o600 },
    );
    for (const receipt of ready) {
      expect(
        artwork.some(
          (entry) =>
            entry.event === "artwork.finished" &&
            entry.job_id === receipt.id &&
            entry.reason === "ready" &&
            typeof entry.duration_ms === "number",
        ),
      ).toBe(true);
    }
    expect(
      artwork.some(
        (entry) =>
          entry.event === "artwork.image_finished" && entry.reason === "loaded",
      ),
    ).toBe(true);
    expect(http.length).toBeGreaterThanOrEqual(ready.length * 2);
    for (const entry of http)
      expect(
        responses.some(
          (response) =>
            response.request === entry.request_id &&
            response.status === entry.status,
        ),
      ).toBe(true);
    await writeFile("/evidence/client.log", text);
    await writeFile(
      "/evidence/artwork-canaries.json",
      JSON.stringify(canaries),
      { mode: 0o600 },
    );
    await writeFile(
      "/evidence/artwork-correlations.json",
      JSON.stringify({
        http,
        ready: ready.map(({ id }) => id),
        enqueues,
        requests: requests.length,
      }),
    );
    return {
      requests: requests.length,
      enqueues: enqueues.length,
      correlatedHTTP: http.length,
      consoleSnapshots: logs.length,
    };
  }
  return {
    observe,
    captureSession,
    enqueues,
    requests,
    responses,
    settle,
    readyJobs: () =>
      artwork
        .filter(
          (entry) =>
            entry.event === "artwork.finished" && entry.reason === "ready",
        )
        .map((entry) => entry.job_id),
    verify,
    phase: (next: string) => {
      phase = next;
    },
  };
}
