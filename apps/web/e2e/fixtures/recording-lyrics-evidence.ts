import { expect, type BrowserContext, type Request } from "@playwright/test";
import { captureConsolePrivacy } from "./console-privacy";
import { lyricDeadline } from "./recording-lyrics-state";

const uuid =
  /^[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/;

export async function recordingLyricsEvidence(
  context: BrowserContext,
  origin: string,
  privateValues: string[],
) {
  const pending = new Set<Promise<void>>();
  const captures: string[] = [];
  const workflow = new Map<
    string,
    { started: number; finished: number; reason: string }
  >();
  const allowed = new WeakSet<Request>();
  const intentional = new WeakSet<Request>();
  const replies = new WeakMap<Request, number>();
  const failures: {
    phase: number;
    kind: number;
    method: number;
    resource: number;
    status: number;
    cancelled: number;
  }[] = [];
  const preparation = new Set<number>();
  const privateFailures: {
    url: string;
    referrer: string;
    frame: string;
    resource: string;
    phase: number;
  }[] = [];
  const chromium = context.browser()?.browserType().name() === "chromium";
  let armed = false;
  const counts = {
    requests: 0,
    foreign: 0,
    forbidden: 0,
    audio: 0,
    prepare: 0,
    downloads: 0,
    implicitWrites: 0,
    lyrics: 0,
    absent: 0,
    downloadReads: 0,
    pageErrors: 0,
    consoleBytes: 0,
    snapshots: 0,
    privateConsole: 0,
    captureFailures: 0,
    javascriptErrors: 0,
    unexpectedRequestFailures: 0,
    cancelledLyrics: 0,
    preparationCancellations: 0,
  };
  context.on("page", (page) => page.on("pageerror", () => counts.pageErrors++));
  for (const page of context.pages())
    page.on("pageerror", () => counts.pageErrors++);
  context.on("console", (message) => {
    if (message.type() === "error" && message.args().length > 0)
      counts.javascriptErrors++;
  });
  context.on("request", (request) => {
    const target = new URL(request.url());
    let rejected = ++counts.requests > 250;
    if (target.origin !== origin) {
      counts.foreign++;
      rejected = true;
    }
    if (
      request.resourceType() === "media" ||
      /\/audio(?:\/|$)/.test(target.pathname)
    ) {
      counts.audio++;
      rejected = true;
    }
    if (/\/(?:preparations?|resolve)(?:\/|$)/.test(target.pathname)) {
      counts.prepare++;
      rejected = true;
    }
    if (/\/downloads?(?:\/|$)/.test(target.pathname)) {
      if (
        request.method() === "GET" &&
        /^\/api\/v1\/recordings\/[^/]+\/download$/.test(target.pathname)
      )
        counts.downloadReads++;
      else {
        counts.downloads++;
        rejected = true;
      }
    }
    if (armed && !["GET", "HEAD", "OPTIONS"].includes(request.method())) {
      if (
        request.method() === "POST" &&
        /^\/api\/v1\/recordings\/[^/]+\/lyrics$/.test(target.pathname)
      )
        counts.lyrics++;
      else {
        counts.implicitWrites++;
        rejected = true;
      }
    }
    if (rejected) counts.forbidden++;
    else allowed.add(request);
  });
  await context.route("**/*", async (route) => {
    if (allowed.has(route.request())) await route.continue();
    else await route.abort("blockedbyclient");
  });
  context.on("requestfailed", (request) => {
    if (privateFailures.length < 12) {
      let frame = "";
      try {
        frame = request.frame().url();
      } catch {}
      privateFailures.push({
        url: request.url().slice(0, 2048),
        referrer: (request.headers().referer ?? "").slice(0, 2048),
        frame: frame.slice(0, 2048),
        resource: request.resourceType(),
        phase: armed ? 1 : 0,
      });
    }
    const target = new URL(request.url());
    const status = replies.get(request) ?? 0;
    const text = request.failure()?.errorText ?? "";
    const setup =
      target.pathname === "/api/v1/setup" &&
      request.method() === "POST" &&
      status === 201;
    const session =
      target.pathname === "/api/v1/me" &&
      request.method() === "GET" &&
      status === 401;
    const kind = setup
      ? 1
      : session
        ? 2
        : /\/lyrics$/.test(target.pathname)
          ? 3
          : 0;
    if (failures.length < 12)
      failures.push({
        phase: armed ? 1 : 0,
        kind,
        method: ["GET", "POST", "PUT", "DELETE"].indexOf(request.method()) + 1,
        resource:
          ["fetch", "xhr", "document", "script", "image"].indexOf(
            request.resourceType(),
          ) + 1,
        status,
        cancelled: /abort|cancel/i.test(text) ? 1 : 0,
      });
    else counts.captureFailures++;
    if (
      !armed &&
      chromium &&
      kind > 0 &&
      kind < 3 &&
      !preparation.has(kind) &&
      target.origin === origin &&
      !target.search &&
      request.resourceType() === "fetch" &&
      text === "net::ERR_ABORTED"
    ) {
      preparation.add(kind);
      counts.preparationCancellations++;
      return;
    }
    if (
      intentional.has(request) &&
      /abort|cancel/i.test(request.failure()?.errorText ?? "")
    )
      counts.cancelledLyrics++;
    else counts.unexpectedRequestFailures++;
  });
  context.on("response", (response) => {
    const request = response.request();
    replies.set(request, response.status());
    if (
      request.method() !== "GET" ||
      !/^\/api\/v1\/recordings\/[^/]+\/download$/.test(
        new URL(response.url()).pathname,
      )
    )
      return;
    const check = (async () => {
      const bytes = await response.body();
      if (
        response.status() === 404 &&
        bytes.toString() === '{"error":"Resource not found"}'
      )
        counts.absent++;
      else counts.forbidden++;
    })()
      .catch(() => {
        counts.forbidden++;
      })
      .finally(() => pending.delete(check));
    pending.add(check);
  });
  await captureConsolePrivacy(
    context,
    (text, capture) => {
      counts.consoleBytes += Buffer.byteLength(text);
      if (counts.consoleBytes > 524_288) {
        counts.captureFailures++;
        return;
      }
      captures.push(text);
      if (capture !== "snapshot") return;
      counts.snapshots++;
      try {
        const values: unknown = JSON.parse(text);
        if (!Array.isArray(values) || values[0] !== "[hito]") return;
        const event = values[1] as Record<string, unknown>;
        if (
          event?.event !== "lyrics.started" &&
          event?.event !== "lyrics.finished"
        )
          return;
        const finished = event.event === "lyrics.finished";
        if (
          Object.keys(event).sort().join(",") !==
            (finished
              ? "action,duration_ms,event,reason,request_id"
              : "action,event,request_id") ||
          event.action !== "read" ||
          typeof event.request_id !== "string" ||
          !uuid.test(event.request_id)
        ) {
          counts.captureFailures++;
          return;
        }
        const current = workflow.get(event.request_id) ?? {
          started: 0,
          finished: 0,
          reason: "",
        };
        if (finished) {
          if (
            ![
              "ready",
              "not_found",
              "cancelled",
              "unsupported",
              "unavailable",
              "failed",
              "changed",
              "deadline",
            ].includes(String(event.reason)) ||
            typeof event.duration_ms !== "number" ||
            !Number.isFinite(event.duration_ms) ||
            event.duration_ms < 0
          )
            counts.captureFailures++;
          current.finished++;
          current.reason = String(event.reason);
        } else current.started++;
        workflow.set(event.request_id, current);
      } catch {
        counts.captureFailures++;
      }
    },
    () => counts.captureFailures++,
    pending,
  );
  await context.addInitScript((expected) => {
    if (location.origin === expected)
      localStorage.setItem("hito-log-level", "debug");
  }, origin);
  return {
    counts,
    failures,
    privateFailures,
    arm() {
      armed = true;
    },
    expectCancellation(request: Request) {
      intentional.add(request);
    },
    async finish(reasons: string[]) {
      await lyricDeadline(
        (async () => {
          while (pending.size) await Promise.allSettled([...pending]);
        })(),
      );
      counts.privateConsole = captures.filter((text) =>
        privateValues.some(
          (value) =>
            value &&
            [value, JSON.stringify(value).slice(1, -1)].some((variant) =>
              text.includes(variant),
            ),
        ),
      ).length;
      expect([...workflow.values()].map((item) => item.reason).sort()).toEqual(
        [...reasons].sort(),
      );
      expect(
        [...workflow.values()].every(
          (item) => item.started === 1 && item.finished === 1,
        ),
      ).toBe(true);
      expect(
        counts.foreign +
          counts.forbidden +
          counts.audio +
          counts.prepare +
          counts.downloads +
          counts.implicitWrites +
          counts.pageErrors +
          counts.privateConsole +
          counts.captureFailures +
          counts.javascriptErrors +
          counts.unexpectedRequestFailures,
      ).toBe(0);
      expect(
        counts.requests > 0 &&
          counts.requests <= 250 &&
          counts.snapshots > 0 &&
          counts.downloadReads > 0 &&
          counts.downloadReads === counts.absent,
      ).toBe(true);
      return { counts, failures, workflows: [...workflow.values()] };
    },
  };
}
