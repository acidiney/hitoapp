import { writeFile } from "node:fs/promises";
import type { BrowserContext } from "@playwright/test";
import { captureConsolePrivacy } from "./console-privacy";
import { libraryConsoleBoundary } from "./audiobook-library-console";

const uuid = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/i;
export async function libraryEvidence(context: BrowserContext, origin: string) {
  const privateValues = new Set<string>();
  const pending = new Set<Promise<void>>();
  const consoleBoundary = libraryConsoleBoundary(context, origin);
  const attempts = new Map<
    string,
    { started: number; finished: number; failed: number; account: string }
  >();
  const counts = {
    consoleBytes: 0,
    snapshots: 0,
    privateConsole: 0,
    nativeNetworkDiagnostics: 0,
    invalidLogs: 0,
    pageErrors: 0,
    started: 0,
    finished: 0,
    failed: 0,
  };
  function remember(value: string) {
    if (
      !value ||
      Buffer.byteLength(value) > 16_384 ||
      privateValues.size >= 512
    )
      throw new Error("The owned privacy canary budget was exceeded");
    privateValues.add(value);
    let escaped = value;
    for (let depth = 0; depth < 2; depth++) {
      escaped = JSON.stringify(escaped).slice(1, -1);
      privateValues.add(escaped);
    }
  }
  await captureConsolePrivacy(
    context,
    (text, capture) => {
      counts.consoleBytes += Buffer.byteLength(text);
      if (counts.consoleBytes > 524_288) {
        counts.invalidLogs++;
        return;
      }
      if (capture !== "snapshot") return;
      consoleBoundary.snapshot(text);
      counts.snapshots++;
      try {
        const message: unknown = JSON.parse(text);
        if (!Array.isArray(message) || message[0] !== "[hito]") return;
        const value = message[1] as Record<string, unknown>;
        if (
          !value ||
          typeof value.event !== "string" ||
          !value.event.startsWith("audiobooks.page_")
        )
          return;
        if (
          Object.keys(value).some(
            (key) =>
              ![
                "event",
                "account_id",
                "request_id",
                "reason",
                "duration_ms",
              ].includes(key),
          ) ||
          typeof value.request_id !== "string" ||
          !uuid.test(value.request_id) ||
          typeof value.account_id !== "string" ||
          !uuid.test(value.account_id)
        ) {
          counts.invalidLogs++;
          return;
        }
        if (attempts.size >= 64 && !attempts.has(value.request_id)) {
          counts.invalidLogs++;
          return;
        }
        const attempt = attempts.get(value.request_id) ?? {
          started: 0,
          finished: 0,
          failed: 0,
          account: value.account_id,
        };
        if (attempt.account !== value.account_id) counts.invalidLogs++;
        const kind = value.event.slice("audiobooks.page_".length);
        if (
          kind !== "started" &&
          (typeof value.duration_ms !== "number" ||
            !Number.isFinite(value.duration_ms) ||
            value.duration_ms < 0 ||
            typeof value.reason !== "string")
        )
          counts.invalidLogs++;
        if (kind === "started") {
          attempt.started++;
          counts.started++;
        } else if (kind === "finished") {
          attempt.finished++;
          counts.finished++;
        } else if (kind === "failed") {
          attempt.failed++;
          counts.failed++;
        } else counts.invalidLogs++;
        attempts.set(value.request_id, attempt);
      } catch {
        counts.invalidLogs++;
      }
    },
    () => counts.invalidLogs++,
    pending,
  );
  context.on("page", (page) => page.on("pageerror", () => counts.pageErrors++));
  for (const page of context.pages())
    page.on("pageerror", () => counts.pageErrors++);
  await context.addInitScript((expected) => {
    if (location.origin !== expected) return;
    localStorage.setItem("hito-log-level", "debug");
    const bytes = crypto.getRandomValues(new Uint32Array(4));
    Object.defineProperty(globalThis, "__hitoLibraryDocument", {
      value: Array.from(bytes).join("-"),
    });
  }, origin);
  return {
    counts,
    remember,
    networkProof: consoleBoundary.networkProof,
    async finish() {
      while (pending.size) await Promise.allSettled([...pending]);
      const inspected = consoleBoundary.inspect(privateValues, true);
      counts.privateConsole = inspected.privateConsole;
      counts.nativeNetworkDiagnostics = inspected.nativeNetworkDiagnostics;
      counts.invalidLogs += inspected.invalid;
      await writeFile(
        "/evidence/privacy-findings.json",
        JSON.stringify(inspected.findings),
        { mode: 0o600 },
      );
      for (const attempt of attempts.values())
        if (
          attempt.started !== 1 ||
          attempt.finished !== 1 ||
          attempt.failed > 1
        )
          counts.invalidLogs++;
      const values = [...privateValues];
      if (Buffer.byteLength(JSON.stringify(values)) > 1_048_576)
        throw new Error("The owned privacy artifact exceeded its bound");
      await writeFile("/evidence/canaries.json", JSON.stringify(values), {
        mode: 0o600,
      });
    },
  };
}
export type LibraryEvidence = Awaited<ReturnType<typeof libraryEvidence>>;
