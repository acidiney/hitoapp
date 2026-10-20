import { expect, type BrowserContext, type Request } from "@playwright/test";
import { captureConsolePrivacy } from "./console-privacy";
import { lyricDeadline } from "./recording-lyrics-state";

const uuid =
  /^[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/;

export async function lyricChoiceEvidence(
  context: BrowserContext,
  origin: string,
  privateValues: string[],
) {
  const pending = new Set<Promise<void>>();
  const allowed = new WeakSet<Request>();
  const owned = new Set<string>();
  const captures: string[] = [];
  const negatives: { url: string; native: boolean; kind: number }[] = [];
  const native: { text: string; url: string }[] = [];
  const workflows = new Map<
    string,
    { kind: "list" | "read"; started: number; finished: number; reason: string }
  >();
  const counts = {
    requests: 0,
    foreign: 0,
    forbidden: 0,
    audio: 0,
    prepare: 0,
    downloads: 0,
    implicitWrites: 0,
    lists: 0,
    lyrics: 0,
    downloadReads: 0,
    artwork404: 0,
    absent: 0,
    negativeBodies: 0,
    negativeBodiesUnavailable: 0,
    nativeCorrelations: 0,
    pageErrors: 0,
    requestFailures: 0,
    unexpectedResponses: 0,
    consoleBytes: 0,
    snapshots: 0,
    privateConsole: 0,
    captureFailures: 0,
  };
  let target: string | null = null;
  let armed = false;
  context.on("page", (page) => page.on("pageerror", () => counts.pageErrors++));
  for (const page of context.pages())
    page.on("pageerror", () => counts.pageErrors++);
  context.on("console", (message) => {
    if (message.type() !== "error") return;
    if (native.length >= 24 || Buffer.byteLength(message.text()) > 4096) {
      counts.captureFailures++;
      return;
    }
    native.push({ text: message.text(), url: message.location().url });
  });
  context.on("request", (request) => {
    const url = new URL(request.url());
    let rejected = ++counts.requests > 250;
    if (
      url.origin !== origin ||
      (url.search && url.pathname.includes("/lyrics"))
    ) {
      counts.foreign++;
      rejected = true;
    }
    if (
      request.resourceType() === "media" ||
      /\/audio(?:\/|$)/.test(url.pathname)
    ) {
      counts.audio++;
      rejected = true;
    }
    if (armed && /\/(?:preparations?|resolve)(?:\/|$)/.test(url.pathname)) {
      counts.prepare++;
      rejected = true;
    }
    if (/\/downloads?(?:\/|$)/.test(url.pathname)) {
      if (
        request.method() === "GET" &&
        /^\/api\/v1\/recordings\/[^/]+\/download$/.test(url.pathname)
      )
        counts.downloadReads++;
      else {
        counts.downloads++;
        rejected = true;
      }
    }
    if (url.pathname.endsWith("/lyrics/providers")) {
      if (
        !armed ||
        request.method() !== "GET" ||
        url.pathname !== `/api/v1/recordings/${target}/lyrics/providers` ||
        ++counts.lists > 1
      )
        rejected = true;
    }
    if (armed && !["GET", "HEAD", "OPTIONS"].includes(request.method())) {
      if (
        request.method() !== "POST" ||
        url.pathname !== `/api/v1/recordings/${target}/lyrics` ||
        ++counts.lyrics > 1
      ) {
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
  context.on("requestfailed", () => counts.requestFailures++);
  context.on("response", (response) => {
    if (response.status() < 400) return;
    const work = (async () => {
      const request = response.request();
      const url = new URL(response.url());
      const match = /^\/api\/v1\/recordings\/([^/]+)\/(artwork|download)$/.exec(
        url.pathname,
      );
      if (
        armed ||
        url.origin !== origin ||
        url.search ||
        response.status() !== 404 ||
        request.method() !== "GET" ||
        !match ||
        negatives.length >= 16 ||
        (match[2] === "artwork" && counts.artwork404 >= 8)
      ) {
        counts.unexpectedResponses++;
        return;
      }
      const headers = await response.allHeaders();
      const size = Number(headers["content-length"]);
      if (
        !headers["cache-control"]?.includes("no-store") ||
        !headers["content-type"]?.includes("application/json") ||
        !uuid.test(headers["x-request-id"] ?? "") ||
        !Number.isSafeInteger(size) ||
        size < 1 ||
        size > 256
      ) {
        counts.captureFailures++;
        return;
      }
      const bytes = await lyricDeadline(response.body().catch(() => null));
      if (bytes === null) counts.negativeBodiesUnavailable++;
      else if (
        bytes.length !== size ||
        bytes.toString() !== '{"error":"Resource not found"}'
      ) {
        counts.captureFailures++;
        return;
      } else counts.negativeBodies++;
      if (match[2] === "artwork") counts.artwork404++;
      else counts.absent++;
      negatives.push({
        url: response.url(),
        native: false,
        kind: match[2] === "artwork" ? 1 : 2,
      });
    })()
      .catch(() => {
        counts.captureFailures++;
      })
      .finally(() => pending.delete(work));
    pending.add(work);
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
          ![
            "lyrics.started",
            "lyrics.finished",
            "lyrics.providers.started",
            "lyrics.providers.finished",
          ].includes(String(event?.event))
        )
          return;
        const finished = String(event.event).endsWith("finished");
        const kind = String(event.event).startsWith("lyrics.providers.")
          ? "list"
          : "read";
        if (
          Object.keys(event).sort().join(",") !==
            (finished
              ? "action,duration_ms,event,reason,request_id"
              : "action,event,request_id") ||
          event.action !== kind ||
          typeof event.request_id !== "string" ||
          !uuid.test(event.request_id)
        ) {
          counts.captureFailures++;
          return;
        }
        const workflow = workflows.get(event.request_id) ?? {
          kind,
          started: 0,
          finished: 0,
          reason: "",
        };
        if (workflow.kind !== kind) counts.captureFailures++;
        if (finished) {
          if (
            event.reason !== (kind === "list" ? "read" : "ready") ||
            typeof event.duration_ms !== "number" ||
            !Number.isFinite(event.duration_ms) ||
            event.duration_ms < 0
          )
            counts.captureFailures++;
          workflow.finished++;
          workflow.reason = String(event.reason);
        } else workflow.started++;
        workflows.set(event.request_id, workflow);
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
    own(ids: string[]) {
      for (const id of ids) {
        if (!uuid.test(id)) throw new Error("Owned choice identity is invalid");
        owned.add(id);
      }
    },
    arm(recording: string) {
      if (!owned.has(recording))
        throw new Error("Owned choice recording is absent");
      target = recording;
      armed = true;
    },
    async finish() {
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
      for (const negative of negatives) {
        const id = /^\/api\/v1\/recordings\/([^/]+)\//.exec(
          new URL(negative.url).pathname,
        )?.[1];
        if (!id || !owned.has(id)) counts.unexpectedResponses++;
      }
      for (const message of native) {
        const response = negatives.find(
          (item) =>
            !item.native &&
            item.url === message.url &&
            message.text ===
              "Failed to load resource: the server responded with a status of 404 (Not Found)",
        );
        if (!response) counts.captureFailures++;
        else {
          response.native = true;
          counts.nativeCorrelations++;
        }
      }
      expect(counts.nativeCorrelations).toBe(negatives.length);
      expect(
        [...workflows.values()].sort((a, b) => a.kind.localeCompare(b.kind)),
      ).toEqual([
        { kind: "list", started: 1, finished: 1, reason: "read" },
        { kind: "read", started: 1, finished: 1, reason: "ready" },
      ]);
      expect(
        counts.foreign +
          counts.forbidden +
          counts.audio +
          counts.prepare +
          counts.downloads +
          counts.implicitWrites +
          counts.pageErrors +
          counts.requestFailures +
          counts.unexpectedResponses +
          counts.privateConsole +
          counts.captureFailures,
      ).toBe(0);
      expect(
        counts.requests > 0 &&
          counts.requests <= 250 &&
          counts.lists === 1 &&
          counts.lyrics === 1 &&
          counts.downloadReads > 0 &&
          counts.absent === counts.downloadReads &&
          counts.artwork404 <= 8 &&
          negatives.length <= 16 &&
          counts.snapshots > 0,
      ).toBe(true);
      return {
        counts,
        workflows: [...workflows.values()],
        negatives: negatives.map((item) => ({
          kind: item.kind,
          native: item.native,
        })),
      };
    },
  };
}
