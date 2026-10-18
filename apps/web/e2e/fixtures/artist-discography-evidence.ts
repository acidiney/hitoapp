import type { BrowserContext, Request } from "@playwright/test";
import { captureConsolePrivacy } from "./console-privacy";

const uuid = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;

export async function artistDiscographyEvidence(
  context: BrowserContext,
  origin: string,
  privateValues: string[],
) {
  const pending = new Set<Promise<void>>();
  const texts = new Set<string>();
  const statusRequests = new WeakSet<Request>();
  const catalogRequests = new WeakSet<Request>();
  let target: { account: string; recording: string } | null = null;
  let catalogPath: string | null = null;
  const started = new Map<string, { family: string; action: string }>();
  const finished = new Set<string>();
  const acceptedReview = {
    load: 0,
    search: 0,
    preview: 0,
    confirm: 0,
    profile: 0,
  };
  const acceptedCatalog = { browse: 0, more: 0 };
  const counts = {
    requests: 0,
    armedRequests: 0,
    preparatoryWrites: 0,
    unexpectedWrites: 0,
    foreign: 0,
    audio: 0,
    download: 0,
    downloadStatusReads: 0,
    downloadStatusAbsentReplies: 0,
    invalidDownloadStatus: 0,
    sourceWrites: 0,
    pageErrors: 0,
    consoleBytes: 0,
    privateConsole: 0,
    malformedLogs: 0,
    discographyRequests: 0,
    discographyReplies: 0,
    invalidCatalog: 0,
    catalogBytes: 0,
    bounds: 0,
    drainTimeout: 0,
  };
  const observeError = () => {
    counts.pageErrors++;
  };
  context.on("page", (page) => page.on("pageerror", observeError));
  for (const page of context.pages()) page.on("pageerror", observeError);
  context.on("request", (request) => {
    if (++counts.requests > 300) counts.bounds++;
    const url = new URL(request.url());
    const method = request.method();
    if (["http:", "https:"].includes(url.protocol) && url.origin !== origin)
      counts.foreign++;
    if (
      request.resourceType() === "media" ||
      /\/audio(?:\/|$)/.test(url.pathname)
    )
      counts.audio++;
    if (/\/(?:download|downloads)(?:\/|$)/.test(url.pathname)) {
      if (
        target &&
        url.origin === origin &&
        !url.search &&
        method === "GET" &&
        url.pathname === `/api/v1/recordings/${target.recording}/download` &&
        request.postData() === null &&
        request.headers()["x-hito-request"] === "1" &&
        request.headers()["x-hito-account"] === target.account
      ) {
        counts.downloadStatusReads++;
        statusRequests.add(request);
      } else counts.download++;
    }
    const writing = !["GET", "HEAD", "OPTIONS"].includes(method);
    if (
      writing &&
      /\/(?:sources|source-choice|preparations)(?:\/|$)/.test(url.pathname)
    )
      counts.sourceWrites++;
    if (!catalogPath) {
      if (writing) counts.preparatoryWrites++;
      return;
    }
    counts.armedRequests++;
    if (!writing) return;
    if (
      url.origin !== origin ||
      url.pathname !== catalogPath ||
      url.search ||
      method !== "POST"
    ) {
      counts.unexpectedWrites++;
      return;
    }
    counts.discographyRequests++;
    catalogRequests.add(request);
    try {
      const raw = request.postData();
      if (!raw || Buffer.byteLength(raw) > 8_256)
        throw new Error("Invalid owned catalog envelope");
      const value: unknown = JSON.parse(raw);
      if (
        value === null ||
        typeof value !== "object" ||
        Object.keys(value).sort().join(",") !== "continuation,limit" ||
        !("limit" in value) ||
        value.limit !== 20 ||
        !("continuation" in value) ||
        !(
          value.continuation === null ||
          (typeof value.continuation === "string" &&
            value.continuation.length > 0 &&
            value.continuation.length <= 8192 &&
            /^[\x20-\x7e]+$/.test(value.continuation))
        ) ||
        request.headers()["x-hito-account"] !== target?.account ||
        request.headers()["x-hito-request"] !== "1" ||
        !request.headers()["content-type"]?.startsWith("application/json")
      )
        counts.invalidCatalog++;
    } catch {
      counts.invalidCatalog++;
    }
  });
  context.on("response", (response) => {
    const status = statusRequests.has(response.request());
    const catalog = catalogRequests.has(response.request());
    if (!status && !catalog) return;
    if (pending.size >= 32) {
      counts.bounds++;
      return;
    }
    const checking = (async () => {
      try {
        const maximum = status ? 4096 : 294_912;
        const declared = Number(response.headers()["content-length"] ?? "0");
        if (!Number.isFinite(declared) || declared < 0 || declared > maximum)
          throw new Error("Owned response exceeds its limit");
        const bytes = await response.body();
        if (
          bytes.length > maximum ||
          !response.headers()["content-type"]?.startsWith("application/json")
        )
          throw new Error("Invalid owned response envelope");
        const value: unknown = JSON.parse(bytes.toString());
        if (status) {
          if (
            response.status() === 404 &&
            typeof value === "object" &&
            value !== null &&
            Object.keys(value).length === 1 &&
            "error" in value &&
            value.error === "Resource not found"
          )
            counts.downloadStatusAbsentReplies++;
          else counts.invalidDownloadStatus++;
        } else {
          counts.catalogBytes += bytes.length;
          if (
            response.status() === 200 &&
            response.headers()["cache-control"] === "private, no-store"
          )
            counts.discographyReplies++;
          else counts.invalidCatalog++;
        }
      } catch {
        if (status) counts.invalidDownloadStatus++;
        else counts.invalidCatalog++;
      }
    })().finally(() => pending.delete(checking));
    pending.add(checking);
  });
  await captureConsolePrivacy(
    context,
    (text, capture) => {
      counts.consoleBytes += Buffer.byteLength(text);
      if (counts.consoleBytes <= 524_288) texts.add(text);
      else counts.bounds++;
      if (
        privateValues.some((value) => value.length > 0 && text.includes(value))
      )
        counts.privateConsole++;
      if (capture !== "snapshot") return;
      try {
        const values: unknown = JSON.parse(text);
        if (!Array.isArray(values) || values[0] !== "[hito]") return;
        const event = values[1] as Record<string, unknown> | null;
        if (
          !event ||
          typeof event !== "object" ||
          typeof event.event !== "string"
        )
          return;
        const match =
          /^(artist_review|artist_discography)\.(started|finished)$/.exec(
            event.event,
          );
        if (!match) return;
        const family = match[1];
        const allowed =
          family === "artist_review"
            ? ["event", "action", "request_id", "reason", "duration_ms"]
            : match[2] === "started"
              ? ["event", "action", "request_id"]
              : ["event", "request_id", "reason", "duration_ms", "count"];
        if (
          Object.keys(event).some((key) => !allowed.includes(key)) ||
          typeof event.request_id !== "string" ||
          !uuid.test(event.request_id)
        ) {
          counts.malformedLogs++;
          return;
        }
        if (match[2] === "started") {
          const actions =
            family === "artist_review"
              ? Object.keys(acceptedReview)
              : Object.keys(acceptedCatalog);
          if (
            !actions.includes(String(event.action)) ||
            started.has(event.request_id) ||
            started.size >= 32
          ) {
            counts.malformedLogs++;
            return;
          }
          started.set(event.request_id, {
            family,
            action: String(event.action),
          });
        } else {
          const first = started.get(event.request_id);
          if (
            !first ||
            first.family !== family ||
            finished.has(event.request_id) ||
            event.reason !== "accepted" ||
            typeof event.duration_ms !== "number" ||
            !Number.isFinite(event.duration_ms) ||
            event.duration_ms < 0 ||
            (family === "artist_review" && event.action !== first.action) ||
            (family === "artist_discography" &&
              (typeof event.count !== "number" ||
                !Number.isInteger(event.count) ||
                event.count < 0 ||
                event.count > 20))
          ) {
            counts.malformedLogs++;
            return;
          }
          if (family === "artist_review")
            acceptedReview[first.action as keyof typeof acceptedReview]++;
          else acceptedCatalog[first.action as keyof typeof acceptedCatalog]++;
          finished.add(event.request_id);
        }
      } catch {
        counts.malformedLogs++;
      }
    },
    () => {
      counts.malformedLogs++;
    },
    pending,
  );
  await context.addInitScript((expected) => {
    if (location.origin === expected)
      localStorage.setItem("hito-log-level", "debug");
  }, origin);
  return {
    counts,
    acceptedReview,
    acceptedCatalog,
    watchDownloadStatus(account: string, recording: string) {
      if (target || !uuid.test(account) || !uuid.test(recording))
        throw new Error("Invalid owned observer target");
      target = { account, recording };
    },
    arm(association: string) {
      if (!target || catalogPath || !uuid.test(association))
        throw new Error("Invalid owned catalog observation phase");
      catalogPath = `/api/v1/recordings/${target.recording}/artist-associations/${association}/discography`;
    },
    complete: () =>
      started.size === 10 &&
      finished.size === 10 &&
      acceptedReview.load === 2 &&
      acceptedReview.search === 1 &&
      acceptedReview.preview === 1 &&
      acceptedReview.confirm === 1 &&
      acceptedReview.profile === 2 &&
      acceptedCatalog.browse === 2 &&
      acceptedCatalog.more === 1,
    async finish(releasePrivate = false) {
      let timer: ReturnType<typeof setTimeout> | undefined;
      try {
        await Promise.race([
          (async () => {
            while (pending.size) await Promise.allSettled([...pending]);
          })(),
          new Promise<void>((resolve) => {
            timer = setTimeout(() => {
              counts.drainTimeout++;
              resolve();
            }, 3000);
          }),
        ]);
      } finally {
        clearTimeout(timer);
      }
      for (const text of texts)
        if (
          privateValues.some(
            (value) => value.length > 0 && text.includes(value),
          )
        )
          counts.privateConsole++;
      if (releasePrivate) texts.clear();
    },
  };
}
