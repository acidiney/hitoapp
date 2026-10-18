import { createServer, type ServerResponse } from "node:http";
import type { AddressInfo } from "node:net";
import { test, type BrowserContext, type Page } from "@playwright/test";
import {
  recordingAttributionDeadline,
  recordingAttributionEvidence,
} from "./fixtures/recording-attribution-evidence";

type Attempt = {
  path: string;
  method: "GET" | "POST";
  status: number;
  kind: number;
  disconnect?: boolean;
};
type Scenario = {
  name: string;
  attempts: Attempt[];
  boundary: "before" | "afterHeaders" | "after";
  preparation: number;
  terminal: number;
};
type Evidence = Awaited<ReturnType<typeof recordingAttributionEvidence>>;

const setup: Attempt = {
  path: "/api/v1/setup",
  method: "POST",
  status: 201,
  kind: 5,
};
const me: Attempt = {
  path: "/api/v1/me",
  method: "GET",
  status: 401,
  kind: 14,
};
const scenarios: Scenario[] = [
  {
    name: "accounts separately for the exact cancelled preparation pair",
    attempts: [setup, me],
    boundary: "after",
    preparation: 2,
    terminal: 0,
  },
  {
    name: "keeps a different preparation endpoint terminal",
    attempts: [{ ...me, path: "/api/v1/owned-session", kind: 12 }],
    boundary: "after",
    preparation: 0,
    terminal: 1,
  },
  {
    name: "keeps a different preparation method terminal",
    attempts: [{ ...me, method: "POST" }],
    boundary: "after",
    preparation: 0,
    terminal: 1,
  },
  {
    name: "keeps a different preparation response status terminal",
    attempts: [{ ...me, status: 200 }],
    boundary: "after",
    preparation: 0,
    terminal: 1,
  },
  {
    name: "keeps a preparation query terminal",
    attempts: [{ ...me, path: "/api/v1/me?owned=1" }],
    boundary: "after",
    preparation: 0,
    terminal: 1,
  },
  {
    name: "keeps a noncancelled preparation transport failure terminal",
    attempts: [{ ...me, disconnect: true }],
    boundary: "after",
    preparation: 0,
    terminal: 1,
  },
  {
    name: "keeps an unknown preparation endpoint terminal",
    attempts: [{ ...me, path: "/owned-session", kind: 0 }],
    boundary: "after",
    preparation: 0,
    terminal: 1,
  },
  {
    name: "keeps an exact matching cancellation after the boundary terminal",
    attempts: [me],
    boundary: "before",
    preparation: 0,
    terminal: 1,
  },
  {
    name: "keeps a preparation request failing after the boundary terminal",
    attempts: [me],
    boundary: "afterHeaders",
    preparation: 0,
    terminal: 1,
  },
  {
    name: "keeps duplicate and dropped preparation failures terminal",
    attempts: Array.from({ length: 9 }, () => me),
    boundary: "after",
    preparation: 1,
    terminal: 8,
  },
];

async function ownedPreparationResponses() {
  const body = '{"owned":1}';
  const counts = { requests: 0, held: 0, disconnected: 0, unexpected: 0 };
  let expected: Attempt | null = null;
  let held: ServerResponse | null = null;
  const server = createServer((request, reply) => {
    reply.on("error", () => {});
    if (++counts.requests > 16) {
      counts.unexpected++;
      reply.writeHead(429).end();
    } else if (request.method === "GET" && request.url === "/") {
      reply
        .writeHead(200, { "content-type": "text/html" })
        .end(
          "<!doctype html><title>Owned preparation response fixture</title>",
        );
    } else if (request.method === "GET" && request.url === "/favicon.ico") {
      reply.writeHead(204).end();
    } else if (
      expected &&
      request.method === expected.method &&
      request.url === expected.path
    ) {
      counts.held++;
      held = reply;
      reply.writeHead(expected.status, {
        "content-type": "application/json",
        "content-length": Buffer.byteLength(body),
        "cache-control": "no-store",
      });
      expected = null;
      reply.write(body.slice(0, 1));
    } else {
      counts.unexpected++;
      reply.writeHead(400).end();
    }
  });
  server.requestTimeout = 4000;
  server.headersTimeout = 4000;
  server.maxHeadersCount = 32;
  server.maxConnections = 4;
  server.setTimeout(5000, (socket) => socket.destroy());
  server.on("clientError", (_error, socket) => socket.destroy());
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      resolve();
    });
  });
  server.on("error", () => counts.unexpected++);
  return {
    origin: `http://127.0.0.1:${(server.address() as AddressInfo).port}`,
    counts,
    expect(attempt: Attempt) {
      if (expected) throw new Error("Owned preparation response is pending");
      expected = attempt;
    },
    disconnect() {
      if (!held || held.destroyed || held.writableEnded)
        throw new Error("Owned preparation response is unavailable");
      counts.disconnected++;
      held.destroy();
    },
    async close() {
      const stopped = new Promise<void>((resolve, reject) => {
        server.close((error) =>
          error
            ? reject(new Error("Owned preparation fixture close failed"))
            : resolve(),
        );
      });
      server.closeAllConnections();
      await recordingAttributionDeadline(stopped, 1000);
    },
  };
}

async function characterize(
  page: Page,
  context: BrowserContext,
  scenario: Scenario,
) {
  const checks = {
    setupFailed: 0,
    headers: 0,
    outcomes: 0,
    browserClosed: 0,
    providerClosed: 0,
    drained: 0,
  };
  let phase = 1;
  let fixture: Awaited<ReturnType<typeof ownedPreparationResponses>> | null =
    null;
  let evidence: Evidence | null = null;
  const arm = () => {
    evidence?.armWorkflow();
    phase = 2;
  };
  try {
    if (process.env.PLAYWRIGHT_NO_COPY_PROMPT !== "1")
      throw new Error("Owned preparation browser copy prompt is enabled");
    fixture = await ownedPreparationResponses();
    evidence = await recordingAttributionEvidence(
      context,
      fixture.origin,
      [],
      () => phase,
    );
    await page.goto(fixture.origin);
    if (scenario.boundary === "before") arm();
    for (const attempt of scenario.attempts) {
      fixture.expect(attempt);
      const received = page
        .waitForResponse(
          (reply) =>
            reply.url() === `${fixture!.origin}${attempt.path}` &&
            reply.request().method() === attempt.method,
          { timeout: 2000 },
        )
        .then(
          (reply) => reply.status(),
          () => 0,
        );
      const failed = page
        .waitForEvent("requestfailed", {
          predicate: (request) =>
            request.url() === `${fixture!.origin}${attempt.path}` &&
            request.method() === attempt.method,
          timeout: 2000,
        })
        .then(
          () => true,
          () => false,
        );
      await page.evaluate(({ path, method }) => {
        const state = globalThis as unknown as Record<string, unknown>;
        const controller = new AbortController();
        state.ownedPreparationCancel = () => controller.abort();
        state.ownedPreparationOutcome = 0;
        state.ownedPreparationStatus = 0;
        void fetch(path, { method, signal: controller.signal }).then(
          async (reply) => {
            state.ownedPreparationStatus = reply.status;
            try {
              await reply.text();
              state.ownedPreparationOutcome = 1;
            } catch {
              state.ownedPreparationOutcome = 2;
            }
          },
          () => {
            state.ownedPreparationOutcome = 3;
          },
        );
      }, attempt);
      if ((await received) !== attempt.status)
        throw new Error("Owned preparation response headers do not match");
      await page.waitForFunction(
        (status) =>
          (globalThis as unknown as Record<string, unknown>)
            .ownedPreparationStatus === status,
        attempt.status,
        { timeout: 2000 },
      );
      checks.headers++;
      if (scenario.boundary === "afterHeaders") arm();
      if (attempt.disconnect) fixture.disconnect();
      else
        await page.evaluate(() => {
          const cancel = (globalThis as unknown as Record<string, unknown>)
            .ownedPreparationCancel;
          if (typeof cancel !== "function")
            throw new Error("Owned preparation cancellation is unavailable");
          cancel();
        });
      if (!(await failed))
        throw new Error("Owned preparation request did not fail");
      await page.waitForFunction(
        () =>
          (globalThis as unknown as Record<string, unknown>)
            .ownedPreparationOutcome === 2,
        undefined,
        { timeout: 2000 },
      );
      checks.outcomes++;
    }
    if (scenario.boundary === "after") arm();
  } catch {
    checks.setupFailed = 1;
  } finally {
    try {
      await recordingAttributionDeadline(context.close(), 1500);
      checks.browserClosed = 1;
    } catch {}
    try {
      await fixture?.close();
      checks.providerClosed = fixture ? 1 : 0;
    } catch {}
    try {
      await evidence?.finish();
      checks.drained = evidence ? 1 : 0;
    } catch {}
  }
  const counts = evidence?.counts;
  const records = evidence?.failures ?? [];
  const attempts = scenario.attempts;
  const preparation = counts?.preparationCancellations ?? 0;
  const terminal = counts?.terminalRequestFailures ?? 0;
  const report = JSON.stringify({
    ...checks,
    ...fixture?.counts,
    requestFailures: counts?.requestFailures ?? 0,
    preparation,
    terminal,
    workflowArmed: counts?.workflowArmed ?? 0,
    dropped: counts?.failureRecordsDropped ?? 0,
    failures: records,
  });
  if (Buffer.byteLength(report) > 3800)
    throw new Error("Owned preparation report exceeded its bound");
  if (
    checks.setupFailed ||
    checks.headers !== attempts.length ||
    checks.outcomes !== attempts.length ||
    checks.browserClosed !== 1 ||
    checks.providerClosed !== 1 ||
    checks.drained !== 1 ||
    fixture?.counts.unexpected !== 0 ||
    fixture.counts.held !== attempts.length ||
    counts?.requestFailures !== attempts.length ||
    counts.workflowArmed !== 1 ||
    counts.requestFailures !== preparation + terminal ||
    counts.foreign +
      counts.forbidden +
      counts.audio +
      counts.preparation +
      counts.download +
      counts.invalidStatus +
      counts.pageErrors +
      counts.privateConsole +
      counts.captureFailures +
      counts.drainTimeouts !==
      0 ||
    counts.failureRecordsDropped !== Math.max(0, attempts.length - 8) ||
    preparation !== scenario.preparation ||
    terminal !== scenario.terminal ||
    records.length !== Math.min(8, attempts.length) ||
    records.some((record, index) => {
      const attempt = attempts[index];
      return (
        record.started !== (scenario.boundary === "before" ? 2 : 1) ||
        record.failed !== (scenario.boundary === "after" ? 1 : 2) ||
        record.kind !== attempt.kind ||
        record.method !== (attempt.method === "GET" ? 1 : 2) ||
        record.resource !== 1 ||
        record.cancelled !== (attempt.disconnect ? 0 : 1) ||
        record.status !== attempt.status ||
        record.sameOrigin !== 1 ||
        record.query !== (attempt.path.includes("?") ? 1 : 0)
      );
    })
  )
    throw new Error(
      `Owned preparation characterization failed; counts=${report}`,
    );
  console.info(`Owned preparation characterization counts=${report}`);
}

test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});
test.skip(
  process.env.HITO_RECORDING_ATTRIBUTION_E2E !== "1",
  "Requires the opted-in owned recording preparation characterization",
);

for (const scenario of scenarios)
  test(`recording evidence ${scenario.name}`, async ({
    page,
    context,
    browserName,
  }) => {
    test.skip(
      browserName !== "chromium" &&
        scenario.attempts.some((attempt) => attempt.status === 401),
      "Held 401 preparation controls are characterized only for Chromium; the owned WebKit fixture does not expose their headers before body completion",
    );
    test.setTimeout(20_000);
    page.setDefaultTimeout(2000);
    page.setDefaultNavigationTimeout(2000);
    await characterize(page, context, scenario);
  });
