import type { BrowserContext, Page, Request, Response } from "@playwright/test";
import { captureConsolePrivacy } from "./console-privacy";

const uuid = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;

export async function bookmarkEvidence(
  context: BrowserContext,
  origin: string,
  privateValues: string[],
) {
  const counts = {
    preparation_requests: 0,
    preparation_failures: 0,
    preparation_status_errors: 0,
    preparation_body_failures: 0,
    preparation_response_bytes: 0,
    workflow_requests: 0,
    workflow_response_bytes: 0,
    foreign: 0,
    audio: 0,
    providers: 0,
    sources: 0,
    downloads: 0,
    unexpected_writes: 0,
    lost_failures: 0,
    unexpected_failures: 0,
    invalid_responses: 0,
    response_bytes: 0,
    page_errors: 0,
    console_bytes: 0,
    private_console: 0,
    malformed_console: 0,
    started: 0,
    finished: 0,
    confirmed: 0,
    unconfirmed: 0,
    invalid_phase: 0,
  };
  const pending = new Set<Promise<void>>();
  const captured: string[] = [];
  const phases = new WeakMap<Request, { armed: boolean; allowed: boolean }>();
  const inFlight = new Set<Request>();
  const logRuns = new Map<string, number>();
  const watchedPages = new WeakSet<Page>();
  let armed = false;
  let lost: Request | null = null;
  const onPage = (page: Page) => {
    if (watchedPages.has(page)) return;
    watchedPages.add(page);
    page.on("pageerror", () => counts.page_errors++);
  };
  context.on("page", onPage);
  for (const page of context.pages()) onPage(page);
  const onRequest = (request: Request) => {
    if (armed) counts.workflow_requests++;
    else counts.preparation_requests++;
    const url = new URL(request.url());
    const foreign =
      ["http:", "https:"].includes(url.protocol) && url.origin !== origin;
    const audio =
      request.resourceType() === "media" ||
      /\/audio(?:\/|$)/.test(url.pathname);
    const provider = /\/(?:plugins|community)(?:\/|$)/.test(url.pathname);
    const source =
      /\/(?:sources|source-choice|preparations|resolve|artist-review|artist-associations)(?:\/|$)/.test(
        url.pathname,
      );
    const download = /\/(?:download|downloads)(?:\/|$)/.test(url.pathname);
    const writing = !["GET", "HEAD", "OPTIONS"].includes(request.method());
    const unexpected =
      writing &&
      !(request.method() === "POST" && url.pathname === "/api/v1/sync");
    const budget =
      counts.preparation_requests + counts.workflow_requests > 320 ||
      counts.workflow_requests > 100;
    counts.foreign += Number(foreign);
    counts.audio += Number(audio);
    counts.downloads += Number(download);
    if (armed) {
      counts.providers += Number(provider);
      counts.sources += Number(source);
      counts.unexpected_writes += Number(unexpected);
    }
    inFlight.add(request);
    phases.set(request, {
      armed,
      allowed:
        !budget &&
        !(foreign || audio || download) &&
        !(armed && (provider || source || unexpected)),
    });
  };
  const onFailed = (request: Request) => {
    const phase = phases.get(request);
    inFlight.delete(request);
    if (!phase) {
      counts.invalid_phase++;
      return;
    }
    if (!phase.armed) {
      counts.preparation_failures++;
      return;
    }
    if (request === lost) counts.lost_failures++;
    else counts.unexpected_failures++;
  };
  const onFinished = (request: Request) => {
    if (!phases.has(request)) counts.invalid_phase++;
    inFlight.delete(request);
  };
  const onResponse = (response: Response) => {
    const request = response.request();
    const phase = phases.get(request);
    if (!phase) {
      counts.invalid_phase++;
      return;
    }
    if (response.status() >= 400) {
      if (phase.armed) counts.invalid_responses++;
      else counts.preparation_status_errors++;
    }
    const url = new URL(response.url());
    if (
      !url.pathname.startsWith("/api/") &&
      !request.isNavigationRequest() &&
      request.headers()["x-inertia"] !== "true"
    )
      return;
    const checking = (async () => {
      try {
        const length = response.headers()["content-length"];
        if (
          length !== undefined &&
          (!/^\d+$/.test(length) || Number(length) > 524_288)
        ) {
          counts.invalid_responses++;
          return;
        }
        if (
          response.status() >= 300 &&
          response.status() < 400 &&
          response.status() !== 304
        ) {
          if (length !== "0") counts.invalid_responses++;
          return;
        }
        const body = await response.body();
        counts.response_bytes += body.length;
        if (phase.armed) counts.workflow_response_bytes += body.length;
        else counts.preparation_response_bytes += body.length;
        if (body.length > 524_288 || counts.response_bytes > 2_097_152)
          counts.invalid_responses++;
      } catch {
        if (phase.armed) counts.invalid_responses++;
        else counts.preparation_body_failures++;
      }
    })().finally(() => pending.delete(checking));
    pending.add(checking);
  };
  context.on("request", onRequest);
  context.on("requestfailed", onFailed);
  context.on("requestfinished", onFinished);
  context.on("response", onResponse);
  await captureConsolePrivacy(
    context,
    (text, capture) => {
      counts.console_bytes += Buffer.byteLength(text);
      if (counts.console_bytes <= 524_288) captured.push(text);
      else counts.malformed_console++;
      if (
        privateValues.some((value) => value.length >= 4 && text.includes(value))
      )
        counts.private_console++;
      if (capture !== "snapshot") return;
      try {
        const values: unknown = JSON.parse(text);
        if (!Array.isArray(values) || values[0] !== "[hito]") return;
        const value: unknown = values[1];
        if (typeof value !== "object" || value === null || !("event" in value))
          return;
        const event = value as Record<string, unknown>;
        if (
          event.event !== "audiobooks.bookmark_started" &&
          event.event !== "audiobooks.bookmark_finished"
        )
          return;
        if (
          Object.keys(event).some(
            (key) =>
              ![
                "event",
                "request_id",
                "action",
                "mode",
                "reason",
                "duration_ms",
              ].includes(key),
          ) ||
          typeof event.request_id !== "string" ||
          !uuid.test(event.request_id) ||
          !["audiobook_bookmark_create", "audiobook_bookmark_remove"].includes(
            String(event.action),
          )
        ) {
          counts.malformed_console++;
          return;
        }
        const key = `${event.request_id}:${event.action}`;
        if (event.event === "audiobooks.bookmark_started") {
          counts.started++;
          if (event.mode !== "write" || logRuns.size > 4)
            counts.malformed_console++;
          logRuns.set(key, (logRuns.get(key) ?? 0) + 1);
        } else {
          counts.finished++;
          const active = logRuns.get(key) ?? 0;
          if (
            active < 1 ||
            !["confirmed", "unconfirmed"].includes(String(event.reason)) ||
            typeof event.duration_ms !== "number" ||
            !Number.isFinite(event.duration_ms) ||
            event.duration_ms < 0
          )
            counts.malformed_console++;
          else logRuns.set(key, active - 1);
          if (event.reason === "confirmed") counts.confirmed++;
          if (event.reason === "unconfirmed") counts.unconfirmed++;
        }
      } catch {
        counts.malformed_console++;
      }
    },
    () => counts.malformed_console++,
    pending,
  );
  await context.addInitScript((expectedOrigin) => {
    if (location.origin === expectedOrigin)
      localStorage.setItem("hito-log-level", "debug");
  }, origin);
  return {
    counts,
    arm() {
      if (armed) throw new Error("The bookmark evidence is already armed");
      armed = true;
    },
    armed: (request: Request) => phases.get(request)?.armed === true,
    allowed(request: Request) {
      const phase = phases.get(request);
      if (!phase) counts.invalid_phase++;
      return phase?.allowed === true;
    },
    lose(request: Request) {
      if (lost || !phases.get(request)?.armed)
        throw new Error("The lost acknowledgement must own one armed request");
      lost = request;
    },
    async finish() {
      while (pending.size) await Promise.allSettled([...pending]);
      for (const text of captured)
        if (
          privateValues.some(
            (value) => value.length >= 4 && text.includes(value),
          )
        )
          counts.private_console++;
      if (inFlight.size || [...logRuns.values()].some((value) => value !== 0))
        counts.invalid_phase++;
      context.off("request", onRequest);
      context.off("requestfailed", onFailed);
      context.off("requestfinished", onFinished);
      context.off("response", onResponse);
      context.off("page", onPage);
    },
  };
}
