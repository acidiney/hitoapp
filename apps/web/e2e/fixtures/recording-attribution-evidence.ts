import type { BrowserContext, Request } from "@playwright/test";
import { captureConsolePrivacy } from "./console-privacy";

const uuid = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;

export async function recordingAttributionDeadline<T>(
  work: Promise<T>,
  milliseconds = 5000,
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      work,
      new Promise<never>((_resolve, reject) => {
        timer = setTimeout(
          () => reject(new Error("Owned recording evidence deadline exceeded")),
          milliseconds,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

function requestKind(request: Request): number {
  const path = new URL(request.url()).pathname;
  if (
    request.isNavigationRequest() ||
    request.headers()["x-inertia"] === "true"
  )
    return 1;
  if (/^\/api\/v1\/libraries\/[^/]+\/folders$/.test(path)) return 2;
  if (/^\/api\/v1\/storage\//.test(path)) return 3;
  if (path === "/api/v1/plugins") return 4;
  if (path === "/api/v1/setup") return 5;
  if (/\/(?:jobs|scan)(?:\/|$)/.test(path)) return 6;
  if (/\/download$/.test(path)) return 7;
  if (/\/source-choice$/.test(path)) return 8;
  if (/\/artwork(?:\/|$)/.test(path)) return 9;
  if (/\/plugins\/[^/]+\/search$/.test(path)) return 10;
  if (/\/plugins\/[^/]+\/recordings$/.test(path)) return 11;
  if (path === "/api/v1/me") return 14;
  if (path.startsWith("/api/")) return 12;
  if (path.startsWith("/assets/")) return 13;
  return 0;
}

export async function recordingAttributionEvidence(
  context: BrowserContext,
  origin: string,
  privateValues: string[],
  phase: () => number = () => 0,
) {
  const pending = new Set<Promise<void>>();
  const captures: string[] = [];
  const statuses = new WeakSet<Request>();
  const rejected = new WeakMap<Request, boolean>();
  const started = new WeakMap<Request, number>();
  const replies = new WeakMap<Request, number>();
  const failures: {
    started: number;
    failed: number;
    kind: number;
    method: number;
    resource: number;
    cancelled: number;
    failureCode: number;
    status: number;
    sameOrigin: number;
    query: number;
  }[] = [];
  let recording: { account: string; id: string } | null = null;
  let installation = "";
  let pin: { source: string } | null = null;
  const chromium = context.browser()?.browserType().name() === "chromium";
  const counts = {
    requests: 0,
    foreign: 0,
    forbidden: 0,
    audio: 0,
    preparation: 0,
    download: 0,
    statusReads: 0,
    absentReplies: 0,
    invalidStatus: 0,
    searches: 0,
    adoptions: 0,
    pins: 0,
    pageErrors: 0,
    requestFailures: 0,
    preparationCancellations: 0,
    terminalRequestFailures: 0,
    workflowArmed: 0,
    consoleBytes: 0,
    text: 0,
    snapshots: 0,
    privateConsole: 0,
    captureFailures: 0,
    drainTimeouts: 0,
    failureRecordsDropped: 0,
  };
  const pageError = () => counts.pageErrors++;
  context.on("page", (page) => page.on("pageerror", pageError));
  for (const page of context.pages()) page.on("pageerror", pageError);
  context.on("request", (request) => {
    started.set(request, phase());
    const url = new URL(request.url());
    let denied = ++counts.requests > 250;
    if (["http:", "https:"].includes(url.protocol) && url.origin !== origin) {
      counts.foreign++;
      denied = true;
    }
    if (
      request.resourceType() === "media" ||
      /\/audio(?:\/|$)/.test(url.pathname)
    ) {
      counts.audio++;
      denied = true;
    }
    if (
      /\/(?:preparations?|resolve|community|artist-review|artist-associations)(?:\/|$)/.test(
        url.pathname,
      )
    ) {
      counts.preparation++;
      denied = true;
    }
    if (/\/(?:download|downloads)(?:\/|$)/.test(url.pathname)) {
      if (
        recording &&
        url.origin === origin &&
        !url.search &&
        url.pathname === `/api/v1/recordings/${recording.id}/download` &&
        request.method() === "GET" &&
        request.postData() === null &&
        request.headers()["x-hito-request"] === "1" &&
        request.headers()["x-hito-account"] === recording.account &&
        counts.statusReads < 8
      ) {
        counts.statusReads++;
        statuses.add(request);
      } else {
        counts.download++;
        denied = true;
      }
    }
    if (!["GET", "HEAD", "OPTIONS"].includes(request.method())) {
      if (/\/(?:sources|source-choice)(?:\/|$)/.test(url.pathname)) {
        let valid = false;
        try {
          const body = request.postDataJSON() as Record<string, unknown>;
          valid =
            !!recording &&
            !!pin &&
            counts.pins === 0 &&
            url.pathname ===
              `/api/v1/recordings/${recording.id}/source-choice` &&
            request.method() === "POST" &&
            request.headers()["x-hito-account"] === recording.account &&
            Object.keys(body).sort().join(",") === "request_id,source_id" &&
            typeof body.request_id === "string" &&
            uuid.test(body.request_id) &&
            body.source_id === pin.source;
        } catch {}
        if (valid) counts.pins++;
        else denied = true;
      }
      if (/\/plugins\/[^/]+\/(search|recordings)$/.test(url.pathname)) {
        const kind = url.pathname.endsWith("/search")
          ? "searches"
          : "adoptions";
        counts[kind]++;
        if (
          !installation ||
          request.method() !== "POST" ||
          counts[kind] > 2 ||
          url.pathname !==
            `/api/v1/plugins/${installation}/${kind === "searches" ? "search" : "recordings"}`
        )
          denied = true;
      }
      if (Buffer.byteLength(request.postData() ?? "") > 16_384) denied = true;
    }
    if (denied) counts.forbidden++;
    rejected.set(request, denied);
  });
  context.on("requestfailed", (request) => {
    counts.requestFailures++;
    counts.terminalRequestFailures++;
    if (failures.length >= 8) {
      counts.failureRecordsDropped++;
      return;
    }
    const url = new URL(request.url());
    const errorText = request.failure()?.errorText ?? "";
    failures.push({
      started: started.get(request) ?? 0,
      failed: phase(),
      kind: requestKind(request),
      method:
        ["GET", "POST", "PUT", "DELETE", "HEAD"].indexOf(request.method()) + 1,
      resource:
        ["fetch", "xhr", "document", "script", "stylesheet", "image"].indexOf(
          request.resourceType(),
        ) + 1,
      cancelled: /abort|cancel/i.test(errorText) ? 1 : 0,
      failureCode:
        errorText === "net::ERR_ABORTED"
          ? 1
          : /abort|cancel/i.test(errorText)
            ? 2
            : 0,
      status: replies.get(request) ?? 0,
      sameOrigin: url.origin === origin ? 1 : 0,
      query: url.search ? 1 : 0,
    });
  });
  await context.route("**/*", async (route) => {
    const request = route.request();
    if (rejected.get(request) !== false) {
      if (!rejected.has(request)) counts.forbidden++;
      await route.abort("blockedbyclient");
    } else await route.continue();
  });
  context.on("response", (response) => {
    replies.set(response.request(), response.status());
    if (!statuses.has(response.request())) return;
    const check = (async () => {
      try {
        const body = await response.body();
        if (
          body.length <= 4096 &&
          response.status() === 404 &&
          response.headers()["content-type"]?.startsWith("application/json") &&
          body.toString() === '{"error":"Resource not found"}'
        )
          counts.absentReplies++;
        else counts.invalidStatus++;
      } catch {
        counts.invalidStatus++;
      }
    })().finally(() => pending.delete(check));
    pending.add(check);
  });
  await captureConsolePrivacy(
    context,
    (text, capture) => {
      counts.consoleBytes += Buffer.byteLength(text);
      counts[capture === "text" ? "text" : "snapshots"]++;
      if (counts.consoleBytes <= 524_288) captures.push(text);
      else counts.captureFailures++;
      if (privateValues.some((value) => value && text.includes(value)))
        counts.privateConsole++;
    },
    () => counts.captureFailures++,
    pending,
  );
  await context.addInitScript((expectedOrigin) => {
    if (location.origin === expectedOrigin)
      localStorage.setItem("hito-log-level", "debug");
  }, origin);
  return {
    counts,
    failures,
    armWorkflow() {
      if (counts.workflowArmed || phase() !== 1)
        throw new Error("Invalid owned recording preparation boundary");
      counts.workflowArmed = 1;
      if (!chromium) return;
      let setup = false;
      let session = false;
      for (const failure of failures) {
        if (
          failure.started !== 1 ||
          failure.failed !== 1 ||
          failure.resource !== 1 ||
          failure.cancelled !== 1 ||
          failure.failureCode !== 1 ||
          failure.sameOrigin !== 1 ||
          failure.query !== 0
        )
          continue;
        if (
          !setup &&
          failure.kind === 5 &&
          failure.method === 2 &&
          failure.status === 201
        )
          setup = true;
        else if (
          !session &&
          failure.kind === 14 &&
          failure.method === 1 &&
          failure.status === 401
        )
          session = true;
        else continue;
        counts.preparationCancellations++;
        counts.terminalRequestFailures--;
      }
    },
    watchInstallation(id: string) {
      if (installation || !uuid.test(id))
        throw new Error("Invalid owned provider observer");
      installation = id;
    },
    watchRecording(account: string, id: string) {
      if (recording || !uuid.test(account) || !uuid.test(id))
        throw new Error("Invalid owned recording observer");
      recording = { account, id };
    },
    allowPin(source: string) {
      if (pin || !uuid.test(source))
        throw new Error("Invalid owned source observer");
      pin = { source };
    },
    async finish() {
      try {
        if (counts.drainTimeouts)
          throw new Error("Owned recording evidence drain timed out");
        await recordingAttributionDeadline(
          (async () => {
            while (pending.size) await Promise.allSettled([...pending]);
          })(),
        );
      } catch {
        counts.drainTimeouts = 1;
        throw new Error("Owned recording evidence drain timed out");
      } finally {
        counts.privateConsole = captures.filter((text) =>
          privateValues.some((value) => value && text.includes(value)),
        ).length;
      }
    },
  };
}
