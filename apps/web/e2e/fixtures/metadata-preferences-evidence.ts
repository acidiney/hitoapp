import { captureConsolePrivacy } from "./console-privacy";
import type { BrowserContext, Page } from "@playwright/test";

const uuid = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;

export async function preferencesEvidence(
  origin: string,
  privateValues: string[],
  phase: () => string = () => "unspecified browser phase",
) {
  const pending = new Set<Promise<void>>();
  const evidence = {
    failed: false,
    bytes: 0,
    snapshots: 0,
    started: 0,
    acceptedSaves: 0,
    requests: 0,
    foreign: 0,
    audio: 0,
    download: 0,
    catalog: 0,
    sourceWrites: 0,
    pageErrors: 0,
  };
  const pageErrorDetails: {
    name: string;
    phase: string;
    document: "blank" | "owned" | "other";
    messageCode:
      | "local_storage_access_denied"
      | "security_operation_insecure"
      | "unrecognized";
    structure: Record<string, boolean | number>;
    hints: Record<string, boolean>;
    frames: { line: number; column: number }[];
  }[] = [];
  const errorNames = new Set([
    "Error",
    "TypeError",
    "ReferenceError",
    "SyntaxError",
    "RangeError",
    "URIError",
    "EvalError",
    "AggregateError",
    "AbortError",
    "DOMException",
    "SecurityError",
    "NotAllowedError",
    "InvalidStateError",
    "NotFoundError",
    "DataCloneError",
    "QuotaExceededError",
    "InvalidAccessError",
  ]);
  function pageError(error: Error, page: Page) {
    evidence.pageErrors++;
    if (pageErrorDetails.length >= 4) return;
    const message = error.message.slice(0, 4096).toLowerCase();
    let document: "blank" | "owned" | "other" = "other";
    const currentUrl = page.url();
    if (currentUrl === "about:blank") document = "blank";
    else {
      try {
        if (new URL(currentUrl).origin === origin) document = "owned";
      } catch {}
    }
    const localStorageDenied =
      message.includes("localstorage") &&
      (message.includes("access is denied") ||
        message.includes("denied access") ||
        message.includes("insecure"));
    const frames: { line: number; column: number }[] = [];
    for (const match of (error.stack ?? "")
      .slice(0, 32768)
      .matchAll(/\/assets\/app\.js:(\d{1,8}):(\d{1,8})\b/g)) {
      frames.push({ line: Number(match[1]), column: Number(match[2]) });
      if (frames.length === 4) break;
    }
    pageErrorDetails.push({
      name: errorNames.has(error.name) ? error.name : "OtherError",
      phase: phase().slice(0, 120),
      document,
      messageCode: localStorageDenied
        ? "local_storage_access_denied"
        : message === "the operation is insecure."
          ? "security_operation_insecure"
          : "unrecognized",
      structure: {
        messageLength: error.message.length,
        nameLength: error.name.length,
        stackLength: error.stack?.length ?? 0,
        isError: error instanceof Error,
        ownPropertyCount: Object.getOwnPropertyNames(error).length,
      },
      hints: Object.fromEntries(
        [
          "abort",
          "fetch",
          "focus",
          "null",
          "undefined",
          "syntax",
          "navigation",
          "signal",
          "network",
          "disposed",
          "closed",
          "cancel",
          "timeout",
          "inertia",
          "update",
          "render",
          "localstorage",
          "storage",
          "denied",
          "access",
          "security",
          "document",
          "window",
        ].map((hint) => [hint, message.includes(hint)]),
      ),
      frames,
    });
  }
  const allowed = new Set([
    "event",
    "action",
    "request_id",
    "reason",
    "duration_ms",
  ]);
  return {
    evidence,
    pageErrorDetails,
    async observe(context: BrowserContext) {
      context.on("page", (page) =>
        page.on("pageerror", (error) => pageError(error, page)),
      );
      for (const page of context.pages())
        page.on("pageerror", (error) => pageError(error, page));
      context.on("request", (request) => {
        if (++evidence.requests > 400) evidence.failed = true;
        const url = new URL(request.url());
        if (["http:", "https:"].includes(url.protocol) && url.origin !== origin)
          evidence.foreign++;
        if (
          request.resourceType() === "media" ||
          /\/audio(?:\/|$)/.test(url.pathname)
        )
          evidence.audio++;
        if (/\/(?:download|downloads)(?:\/|$)/.test(url.pathname))
          evidence.download++;
        if (/\/(?:catalog|artist-profiles)(?:\/|$)/.test(url.pathname))
          evidence.catalog++;
        if (
          !["GET", "HEAD", "OPTIONS"].includes(request.method()) &&
          /\/(?:sources|source-choice|preparations)(?:\/|$)/.test(url.pathname)
        )
          evidence.sourceWrites++;
      });
      await captureConsolePrivacy(
        context,
        (text, capture) => {
          evidence.bytes += Buffer.byteLength(text);
          if (
            evidence.bytes > 524_288 ||
            privateValues.some((value) => text.includes(value))
          )
            evidence.failed = true;
          if (capture !== "snapshot") return;
          evidence.snapshots++;
          try {
            const values: unknown = JSON.parse(text);
            if (!Array.isArray(values) || values[0] !== "[hito]") return;
            const fields: unknown = values[1];
            if (
              typeof fields !== "object" ||
              fields === null ||
              !("event" in fields)
            )
              return;
            const event = fields as Record<string, unknown>;
            if (event.event === "settings.section") {
              if (
                Object.keys(event).some(
                  (key) => !["event", "mode"].includes(key),
                ) ||
                typeof event.mode !== "string"
              )
                evidence.failed = true;
              return;
            }
            if (
              event.event !== "metadata_preferences.started" &&
              event.event !== "metadata_preferences.finished"
            )
              return;
            if (
              Object.keys(event).some((key) => !allowed.has(key)) ||
              !["load", "save"].includes(String(event.action)) ||
              typeof event.request_id !== "string" ||
              !uuid.test(event.request_id)
            )
              evidence.failed = true;
            if (event.event === "metadata_preferences.started")
              evidence.started++;
            else {
              if (
                !["accepted", "cancelled", "failed"].includes(
                  String(event.reason),
                ) ||
                typeof event.duration_ms !== "number" ||
                !Number.isFinite(event.duration_ms) ||
                event.duration_ms < 0
              )
                evidence.failed = true;
              if (event.action === "save" && event.reason === "accepted")
                evidence.acceptedSaves++;
            }
          } catch {
            evidence.failed = true;
          }
        },
        () => {
          evidence.failed = true;
        },
        pending,
      );
      await context.addInitScript((expectedOrigin) => {
        if (location.origin !== expectedOrigin) return;
        localStorage.setItem("hito-log-level", "debug");
      }, origin);
    },
    async finish() {
      await Promise.allSettled([...pending]);
    },
  };
}
