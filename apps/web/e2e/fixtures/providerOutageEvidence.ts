import { createHash } from "node:crypto";
import { readFile, rename, writeFile } from "node:fs/promises";
import { expect, type BrowserContext, type Page } from "@playwright/test";
import { captureConsolePrivacy } from "./console-privacy";
import type { Schema } from "../../src/shared/schema";

export const outageMessage =
  "An audio service is unavailable. Select the track again to retry.";
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
export const hash = (value: unknown) =>
  createHash("sha256").update(JSON.stringify(value)).digest("hex");

export async function outageControl(
  name: string,
  phase: string,
  extra: object = {},
) {
  const temporary = `/evidence/.${name}.tmp`;
  await writeFile(
    temporary,
    JSON.stringify({ run_id: process.env.HITO_OUTAGE_RUN_ID, phase, ...extra }),
  );
  await rename(temporary, `/evidence/${name}.json`);
}

export async function waitOutageControl(
  name: string,
  phase: string,
  milliseconds: number,
) {
  const started = performance.now();
  while (performance.now() - started < milliseconds) {
    try {
      const bytes = await readFile(`/evidence/${name}.json`);
      if (bytes.length > 4096)
        throw new Error("Owned control exceeded its bound");
      const value = JSON.parse(bytes.toString()) as Record<string, unknown>;
      if (
        value.run_id !== process.env.HITO_OUTAGE_RUN_ID ||
        value.phase !== phase
      )
        throw new Error("Owned control identity changed");
      return value;
    } catch (error) {
      if (
        !(
          error &&
          typeof error === "object" &&
          "code" in error &&
          error.code === "ENOENT"
        )
      )
        throw error;
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error("Owned control deadline exceeded");
}

export async function providerOutageEvidence(
  context: BrowserContext,
  page: Page,
  origin: string,
) {
  const pending = new Set<Promise<void>>();
  const privateValues = new Set<string>();
  const captures: string[] = [];
  let captureBytes = 0;
  let armed = false;
  let account = "";
  const heard = new Set<string>();
  const requests: { recording: string; body: string }[] = [];
  const receipts: Schema<"SourceResolution">[] = [];
  const completions: Record<string, unknown>[] = [];
  const assets: Record<string, string> = {};
  const counts = {
    foreign: 0,
    pageErrors: 0,
    callbacks: 0,
    bounds: 0,
    privateConsole: 0,
    unexpectedWrites: 0,
    downloadWrites: 0,
    audioResponses: 0,
    invalidAudio: 0,
    progressWrites: 0,
  };
  const inspect = (text: string) => {
    if ([...privateValues].some((value) => text.includes(value)))
      counts.privateConsole++;
  };
  await captureConsolePrivacy(
    context,
    (text, capture) => {
      if (
        captureBytes + Buffer.byteLength(text) > 2_000_000 ||
        captures.length >= 1024 ||
        Buffer.byteLength(text) > 65_536
      ) {
        counts.bounds++;
        return;
      }
      captureBytes += Buffer.byteLength(text);
      captures.push(text);
      inspect(text);
      if (capture === "snapshot") {
        try {
          const values: unknown = JSON.parse(text);
          if (
            Array.isArray(values) &&
            values[0] === "[hito]" &&
            values[1]?.event === "playback.source_resolution_completed"
          )
            completions.push(values[1]);
        } catch {
          counts.callbacks++;
        }
      }
    },
    () => counts.callbacks++,
    pending,
  );
  page.on("pageerror", () => counts.pageErrors++);
  context.on("request", (request) => {
    try {
      const url = new URL(request.url());
      if (!url.protocol.startsWith("http")) return;
      if (url.origin !== origin) {
        counts.foreign++;
        return;
      }
      if (!armed || ["GET", "HEAD"].includes(request.method())) return;
      if (url.search || url.hash) {
        counts.unexpectedWrites++;
        return;
      }
      if (/\/download(?:\/|$)/.test(url.pathname)) {
        counts.downloadWrites++;
        return;
      }
      const raw = request.postData();
      if (
        !raw ||
        Buffer.byteLength(raw) > 16_384 ||
        request.method() !== "POST" ||
        request.headers()["x-hito-request"] !== "1" ||
        request.headers()["x-hito-account"] !== account
      ) {
        counts.unexpectedWrites++;
        return;
      }
      const data = JSON.parse(raw) as Record<string, unknown>;
      const resolve =
        /^\/api\/v1\/recordings\/([0-9a-f-]+)\/sources\/resolve$/.exec(
          url.pathname,
        );
      if (resolve) {
        if (
          Object.keys(data).join() !== "request_id" ||
          typeof data.request_id !== "string" ||
          !uuid.test(data.request_id) ||
          requests.length >= 8
        )
          counts.unexpectedWrites++;
        else requests.push({ recording: resolve[1], body: raw });
      } else if (
        /^\/api\/v1\/recordings\/[0-9a-f-]+\/sources\/refresh$/.test(
          url.pathname,
        ) &&
        data.purpose === "playback" &&
        Object.keys(data).length === 1
      ) {
        return;
      } else if (url.pathname === "/api/v1/sync") {
        const batch = data as Schema<"SyncBatch">;
        if (
          !Array.isArray(batch.operations) ||
          !batch.operations.length ||
          !batch.operations.every(
            (operation) =>
              uuid.test(operation.id) &&
              operation.action.kind === "progress_set" &&
              heard.has(operation.action.recording_id),
          )
        )
          counts.unexpectedWrites++;
        else counts.progressWrites++;
      } else counts.unexpectedWrites++;
    } catch {
      counts.callbacks++;
    }
  });
  context.on("response", (response) => {
    const task = (async () => {
      const url = new URL(response.url());
      if (
        url.origin === origin &&
        ["/assets/app.js", "/assets/app.css"].includes(url.pathname)
      ) {
        const bytes = await response.body();
        if (response.status() !== 200 || bytes.length > 2 * 1024 * 1024) {
          counts.bounds++;
          return;
        }
        assets[url.pathname] = createHash("sha256").update(bytes).digest("hex");
      }
      if (!armed || url.origin !== origin) return;
      if (/^\/api\/v1\/sources\/[0-9a-f-]+\/audio$/.test(url.pathname)) {
        if (
          [200, 206].includes(response.status()) &&
          response.headers()["content-type"]?.startsWith("audio/wav")
        )
          counts.audioResponses++;
        else counts.invalidAudio++;
      }
      if (
        /\/sources\/resolve$/.test(url.pathname) &&
        response.request().method() === "POST"
      ) {
        const bytes = await response.body();
        if (response.status() !== 200 || bytes.length > 16_384) {
          counts.bounds++;
          return;
        }
        const receipt: Schema<"SourceResolution"> = JSON.parse(
          bytes.toString(),
        );
        receipts.push(receipt);
      }
    })()
      .catch((): void => {
        counts.callbacks++;
      })
      .finally(() => pending.delete(task));
    pending.add(task);
  });
  return {
    counts,
    requests,
    receipts,
    completions,
    assets,
    register(values: string[]) {
      for (const value of values) {
        if (!value || value.length > 4096 || privateValues.size > 256)
          throw new Error("Owned private canary bound exceeded");
        privateValues.add(value);
      }
      for (const text of captures) inspect(text);
    },
    arm(user: string, recordings: string[]) {
      account = user;
      for (const id of recordings) heard.add(id);
      armed = true;
    },
    async drain() {
      let timer: ReturnType<typeof setTimeout> | undefined;
      try {
        await Promise.race([
          (async () => {
            while (pending.size) await Promise.all([...pending]);
          })(),
          new Promise<never>((_resolve, reject) => {
            timer = setTimeout(
              () => reject(new Error("Owned observer drain timed out")),
              5_000,
            );
          }),
        ]);
      } finally {
        clearTimeout(timer);
      }
    },
    async serverLogCheck() {
      const registry = JSON.stringify([...privateValues]);
      if (Buffer.byteLength(registry) > 65_536)
        throw new Error("Owned private registry exceeded its bound");
      await writeFile("/control/private-canaries.json", registry, {
        mode: 0o600,
      });
      await outageControl("log-check-ready", "log_check_ready", { assets });
      const result = await waitOutageControl(
        "logs-checked",
        "logs_checked",
        30_000,
      );
      expect(
        result.private_matches === 0 &&
          result.malformed === 0 &&
          result.unavailable_commits === 2 &&
          result.ready_commits === 1,
      ).toBe(true);
      return result;
    },
    summary() {
      expect(
        Object.entries(counts)
          .filter(
            ([key]) => !["audioResponses", "progressWrites"].includes(key),
          )
          .every(([, value]) => value === 0),
      ).toBe(true);
      expect(counts.audioResponses > 0 && counts.progressWrites > 0).toBe(true);
      return {
        counts,
        request_witnesses: requests.map((request) => ({
          sha256: hash(request.body),
          bytes: Buffer.byteLength(request.body),
        })),
        accepted: receipts.map((receipt) => ({
          outcome: receipt.outcome,
          sha256: hash(receipt),
        })),
        console: {
          captures: captures.length,
          bytes: captureBytes,
          sha256: hash(captures),
          private_canaries: [...privateValues].map((value) => ({
            bytes: Buffer.byteLength(value),
            sha256: hash(value),
          })),
        },
      };
    },
  };
}
export type OutageEvidence = Awaited<ReturnType<typeof providerOutageEvidence>>;
