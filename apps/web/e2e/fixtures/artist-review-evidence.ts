import type { BrowserContext, Request } from "@playwright/test";
import { captureConsolePrivacy } from "./console-privacy";

const uuid = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;
const phases = ["load", "search", "preview", "confirm", "profile"];

export async function artistReviewEvidence(
  context: BrowserContext,
  origin: string,
  privateValues: string[],
) {
  const pending = new Set<Promise<void>>();
  const statusRequests = new WeakSet<Request>();
  let downloadStatusTarget: { account: string; path: string } | null = null;
  const started = new Map<string, string>();
  const completed = new Set<string>();
  const accepted = Object.fromEntries(phases.map((phase) => [phase, 0]));
  const counts = {
    requests: 0,
    foreign: 0,
    audio: 0,
    download: 0,
    downloadStatusReads: 0,
    downloadStatusAbsentReplies: 0,
    invalidDownloadStatus: 0,
    sourceWrites: 0,
    pageErrors: 0,
    privateConsole: 0,
    malformedLogs: 0,
    consoleBytes: 0,
    context: 0,
    search: 0,
    preview: 0,
    confirm: 0,
    profile: 0,
    recordingNavigations: 0,
  };
  context.on("page", (page) => page.on("pageerror", () => counts.pageErrors++));
  for (const page of context.pages())
    page.on("pageerror", () => counts.pageErrors++);
  context.on("request", (request) => {
    counts.requests++;
    const url = new URL(request.url());
    if (["http:", "https:"].includes(url.protocol) && url.origin !== origin)
      counts.foreign++;
    if (
      request.resourceType() === "media" ||
      /\/audio(?:\/|$)/.test(url.pathname)
    )
      counts.audio++;
    if (/\/(?:download|downloads)(?:\/|$)/.test(url.pathname)) {
      if (
        downloadStatusTarget &&
        url.origin === origin &&
        url.pathname === downloadStatusTarget.path &&
        !url.search &&
        request.method() === "GET" &&
        request.postData() === null &&
        request.headers()["x-hito-request"] === "1" &&
        request.headers()["x-hito-account"] === downloadStatusTarget.account
      ) {
        counts.downloadStatusReads++;
        statusRequests.add(request);
      } else counts.download++;
    }
    if (
      !["GET", "HEAD", "OPTIONS"].includes(request.method()) &&
      /\/(?:sources|source-choice|preparations)(?:\/|$)/.test(url.pathname)
    )
      counts.sourceWrites++;
    const phase = /\/artist-review\/(context|search|preview|confirm)$/.exec(
      url.pathname,
    )?.[1];
    if (phase && request.method() === "POST")
      counts[phase as "context" | "search" | "preview" | "confirm"]++;
    if (
      /\/artist-associations\/[^/]+\/profile$/.test(url.pathname) &&
      request.method() === "GET"
    )
      counts.profile++;
    if (
      /^\/recordings\/[^/]+$/.test(url.pathname) &&
      (request.isNavigationRequest() ||
        request.headers()["x-inertia"] === "true")
    )
      counts.recordingNavigations++;
  });
  context.on("response", (response) => {
    if (!statusRequests.has(response.request())) return;
    const checking = (async () => {
      try {
        const body = await response.body();
        const value: unknown = JSON.parse(body.toString());
        if (
          response.status() === 404 &&
          response.headers()["content-type"]?.startsWith("application/json") &&
          body.length <= 4096 &&
          typeof value === "object" &&
          value !== null &&
          Object.keys(value).length === 1 &&
          "error" in value &&
          value.error === "Resource not found"
        )
          counts.downloadStatusAbsentReplies++;
        else counts.invalidDownloadStatus++;
      } catch {
        counts.invalidDownloadStatus++;
      }
    })().finally(() => pending.delete(checking));
    pending.add(checking);
  });
  await captureConsolePrivacy(
    context,
    (text, capture) => {
      counts.consoleBytes += Buffer.byteLength(text);
      if (privateValues.some((value) => text.includes(value)))
        counts.privateConsole++;
      if (capture !== "snapshot") return;
      try {
        const message: unknown = JSON.parse(text);
        if (!Array.isArray(message) || message[0] !== "[hito]") return;
        const value: unknown = message[1];
        if (typeof value !== "object" || value === null || !("event" in value))
          return;
        const event = value as Record<string, unknown>;
        if (
          !["artist_review.started", "artist_review.finished"].includes(
            String(event.event),
          )
        )
          return;
        if (
          Object.keys(event).some(
            (key) =>
              ![
                "event",
                "action",
                "request_id",
                "reason",
                "duration_ms",
              ].includes(key),
          ) ||
          !phases.includes(String(event.action)) ||
          typeof event.request_id !== "string" ||
          !uuid.test(event.request_id)
        ) {
          counts.malformedLogs++;
          return;
        }
        if (event.event === "artist_review.started") {
          if (started.has(event.request_id)) counts.malformedLogs++;
          started.set(event.request_id, String(event.action));
        } else {
          if (
            started.get(event.request_id) !== event.action ||
            completed.has(event.request_id) ||
            event.reason !== "accepted" ||
            typeof event.duration_ms !== "number" ||
            !Number.isFinite(event.duration_ms) ||
            event.duration_ms < 0
          )
            counts.malformedLogs++;
          else accepted[String(event.action)]++;
          completed.add(event.request_id);
        }
      } catch {
        counts.malformedLogs++;
      }
    },
    () => counts.malformedLogs++,
    pending,
  );
  await context.addInitScript((expectedOrigin) => {
    if (location.origin === expectedOrigin)
      localStorage.setItem("hito-log-level", "debug");
  }, origin);
  return {
    counts,
    accepted,
    watchDownloadStatus(account: string, recording: string) {
      if (downloadStatusTarget || !uuid.test(account) || !uuid.test(recording))
        throw new Error("The owned download status observer target is invalid");
      downloadStatusTarget = {
        account,
        path: `/api/v1/recordings/${recording}/download`,
      };
    },
    complete: () => started.size === 7 && completed.size === 7,
    async finish() {
      await Promise.allSettled([...pending]);
    },
  };
}
