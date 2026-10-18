import type { BrowserContext } from "@playwright/test";
import { captureConsolePrivacy } from "./console-privacy";

const uuid = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;

export async function audiobookGroupingEvidence(
  context: BrowserContext,
  origin: string,
  privateValues: string[],
  phase: () => string,
) {
  const pending = new Set<Promise<void>>();
  const capturedTexts = new Set<string>();
  const counts = {
    consoleBytes: 0,
    snapshots: 0,
    privateConsole: 0,
    malformedLogs: 0,
    pageErrors: 0,
    groupStarted: 0,
    unconfirmed: 0,
    confirmed: 0,
    navigationStarted: 0,
    navigationFinished: 0,
  };
  const errors: {
    phase: string;
    name: string;
    frames: { line: number; column: number }[];
  }[] = [];
  const operationIds = new Set<string>();
  const allowed = new Set([
    "event",
    "action",
    "request_id",
    "mode",
    "reason",
    "duration_ms",
  ]);
  const knownNames = new Set([
    "Error",
    "TypeError",
    "ReferenceError",
    "SyntaxError",
    "SecurityError",
    "AbortError",
  ]);
  function observeError(error: Error) {
    counts.pageErrors++;
    if (errors.length >= 4) return;
    const frames = [
      ...(error.stack ?? "")
        .slice(0, 32768)
        .matchAll(/\/assets\/app\.js:(\d{1,8}):(\d{1,8})\b/g),
    ]
      .slice(0, 4)
      .map((match) => ({ line: Number(match[1]), column: Number(match[2]) }));
    errors.push({
      phase: phase().slice(0, 120),
      name: knownNames.has(error.name) ? error.name : "OtherError",
      frames,
    });
  }
  context.on("page", (page) => page.on("pageerror", observeError));
  for (const page of context.pages()) page.on("pageerror", observeError);
  await captureConsolePrivacy(
    context,
    (text, capture) => {
      counts.consoleBytes += Buffer.byteLength(text);
      if (counts.consoleBytes <= 524_288) capturedTexts.add(text);
      else counts.privateConsole = 1;
      if (privateValues.some((value) => text.includes(value)))
        counts.privateConsole = 1;
      if (capture !== "snapshot") return;
      counts.snapshots++;
      try {
        const message: unknown = JSON.parse(text);
        if (!Array.isArray(message) || message[0] !== "[hito]") return;
        const event = message[1] as Record<string, unknown> | null;
        if (
          event === null ||
          typeof event !== "object" ||
          typeof event.event !== "string" ||
          !/^audiobooks\.(?:group_|navigation_)/.test(event.event)
        )
          return;
        if (
          Object.keys(event).some((key) => !allowed.has(key)) ||
          typeof event.request_id !== "string" ||
          !uuid.test(event.request_id) ||
          !["audiobooks.group", "audiobooks.navigate"].includes(
            String(event.action),
          )
        ) {
          counts.malformedLogs++;
          return;
        }
        operationIds.add(event.request_id);
        if (operationIds.size > 1) counts.malformedLogs++;
        if (event.event === "audiobooks.group_started") {
          counts.groupStarted++;
          if (event.mode !== "write") counts.malformedLogs++;
        } else if (event.event === "audiobooks.navigation_started")
          counts.navigationStarted++;
        else {
          if (
            typeof event.duration_ms !== "number" ||
            !Number.isFinite(event.duration_ms) ||
            event.duration_ms < 0 ||
            typeof event.reason !== "string"
          )
            counts.malformedLogs++;
          if (event.event === "audiobooks.group_finished") {
            if (event.reason === "unconfirmed") counts.unconfirmed++;
            else if (event.reason === "confirmed") counts.confirmed++;
            else counts.malformedLogs++;
          } else if (event.event === "audiobooks.navigation_finished")
            counts.navigationFinished++;
          else counts.malformedLogs++;
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
    errors,
    async finish() {
      while (pending.size) await Promise.allSettled([...pending]);
      for (const text of capturedTexts) {
        if (privateValues.some((value) => text.includes(value))) {
          counts.privateConsole = 1;
          break;
        }
      }
    },
  };
}
