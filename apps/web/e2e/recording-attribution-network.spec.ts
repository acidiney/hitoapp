import { randomUUID } from "node:crypto";
import {
  createServer,
  type IncomingMessage,
  type ServerResponse,
} from "node:http";
import type { AddressInfo } from "node:net";
import { test, type BrowserContext, type Page } from "@playwright/test";
import {
  recordingAttributionDeadline,
  recordingAttributionEvidence,
} from "./fixtures/recording-attribution-evidence";

type Scenario =
  | "preparation"
  | "redirect"
  | "unrouted-unload"
  | "routed-unload";

async function ownedNetworkResponses() {
  const preparation = `/api/v1/recordings/${randomUUID()}/sources/${randomUUID()}/preparation`;
  const audio = "/owned/audio";
  const counts = {
    requests: 0,
    preparations: 0,
    redirects: 0,
    audio: 0,
    foreign: 0,
    unexpected: 0,
  };
  async function listen(
    respond: (request: IncomingMessage, reply: ServerResponse) => void,
  ) {
    const server = createServer((request, reply) => {
      reply.on("error", () => {});
      if (++counts.requests > 16) {
        counts.unexpected++;
        reply.writeHead(429).end();
      } else respond(request, reply);
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
      async close() {
        const stopped = new Promise<void>((resolve, reject) => {
          server.close((error) =>
            error
              ? reject(new Error("Owned network fixture close failed"))
              : resolve(),
          );
        });
        server.closeAllConnections();
        await recordingAttributionDeadline(stopped, 1000);
      },
    };
  }
  const foreign = await listen((request, reply) => {
    if (request.method === "GET" && request.url === "/owned-pixel.svg") {
      counts.foreign++;
      reply
        .writeHead(200, { "content-type": "image/svg+xml" })
        .end('<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>');
    } else {
      counts.unexpected++;
      reply.writeHead(400).end();
    }
  });
  let local: Awaited<ReturnType<typeof listen>>;
  try {
    local = await listen((request, reply) => {
      if (request.method === "GET" && request.url === "/") {
        reply
          .writeHead(200, { "content-type": "text/html" })
          .end(
            '<!doctype html><link rel="icon" href="data:,"><title>Owned network fixture</title>',
          );
      } else if (request.method === "GET" && request.url === preparation) {
        counts.preparations++;
        reply
          .writeHead(200, { "content-type": "application/json" })
          .end('{"owned":1}');
      } else if (request.method === "GET" && request.url === "/owned-image") {
        counts.redirects++;
        reply
          .writeHead(302, { location: `${foreign.origin}/owned-pixel.svg` })
          .end();
      } else if (request.method === "GET" && request.url === audio) {
        counts.audio++;
        reply.writeHead(204).end();
      } else {
        counts.unexpected++;
        reply.writeHead(400).end();
      }
    });
  } catch (error) {
    await foreign.close();
    throw error;
  }
  return {
    origin: local.origin,
    foreign: foreign.origin,
    preparation,
    audio,
    counts,
    async close() {
      const results = await Promise.allSettled([
        local.close(),
        foreign.close(),
      ]);
      if (results.some((result) => result.status === "rejected"))
        throw new Error("Owned network fixtures did not close");
    },
  };
}

async function characterizeNetwork(
  page: Page,
  context: BrowserContext,
  scenario: Scenario,
) {
  const checks = {
    setupFailed: 0,
    outcome: 0,
    observed: 0,
    redirected: 0,
    late: 0,
    browserClosed: 0,
    providerClosed: 0,
    drained: 0,
  };
  let phase = 1;
  let fixture: Awaited<ReturnType<typeof ownedNetworkResponses>> | null = null;
  let evidence: Awaited<
    ReturnType<typeof recordingAttributionEvidence>
  > | null = null;
  try {
    if (process.env.PLAYWRIGHT_NO_COPY_PROMPT !== "1")
      throw new Error("Owned network browser copy prompt is enabled");
    fixture = await ownedNetworkResponses();
    const owned = fixture;
    context.on("request", (request) => {
      checks.observed++;
      const url = new URL(request.url());
      if (url.origin === owned.foreign) checks.redirected++;
      if (url.origin === owned.origin && url.pathname === owned.audio)
        checks.late++;
    });
    evidence = await recordingAttributionEvidence(
      context,
      owned.origin,
      [],
      () => phase,
    );
    await page.goto(owned.origin);
    evidence.armWorkflow();
    phase = 2;
    if (scenario === "preparation") {
      checks.outcome = await recordingAttributionDeadline(
        page.evaluate(async (path) => {
          try {
            const reply = await fetch(path);
            await reply.text();
            return 2;
          } catch {
            return 1;
          }
        }, owned.preparation),
        3000,
      );
    } else if (scenario === "redirect") {
      checks.outcome = await recordingAttributionDeadline(
        page.evaluate(
          () =>
            new Promise<number>((resolve) => {
              const image = new Image();
              image.onload = () => resolve(1);
              image.onerror = () => resolve(2);
              image.src = "/owned-image";
              document.body.append(image);
            }),
        ),
        3000,
      );
    } else {
      await page.evaluate((path) => {
        window.addEventListener(
          "pagehide",
          () => {
            void fetch(path, { keepalive: true }).catch(() => {});
          },
          { once: true },
        );
      }, owned.audio);
      const issued = context
        .waitForEvent("request", {
          predicate: (request) =>
            request.url() === `${owned.origin}${owned.audio}`,
          timeout: 2000,
        })
        .then(
          () => 1,
          () => 0,
        );
      const settled = Promise.race([
        context
          .waitForEvent("requestfinished", {
            predicate: (request) =>
              request.url() === `${owned.origin}${owned.audio}`,
            timeout: 2000,
          })
          .then(
            () => 2,
            () => 0,
          ),
        context
          .waitForEvent("requestfailed", {
            predicate: (request) =>
              request.url() === `${owned.origin}${owned.audio}`,
            timeout: 2000,
          })
          .then(
            () => 1,
            () => 0,
          ),
      ]);
      if (scenario === "unrouted-unload")
        await context.unrouteAll({ behavior: "wait" });
      await page.goto("about:blank");
      if ((await issued) !== 1)
        throw new Error("Owned unload request was not observed");
      checks.outcome = await settled;
    }
  } catch {
    checks.setupFailed = 1;
  } finally {
    phase = 9;
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
  const report = JSON.stringify({
    ...checks,
    provider: fixture?.counts,
    evidence: counts,
    failures: evidence?.failures ?? [],
  });
  if (Buffer.byteLength(report) > 3800)
    throw new Error("Owned network report exceeded its bound");
  if (
    checks.setupFailed ||
    checks.browserClosed !== 1 ||
    checks.providerClosed !== 1 ||
    checks.drained !== 1 ||
    fixture?.counts.unexpected !== 0 ||
    counts?.requests !== checks.observed ||
    counts.foreign !== (scenario === "redirect" ? 1 : 0) ||
    counts.forbidden !== 1 ||
    counts.preparation !== (scenario === "preparation" ? 1 : 0) ||
    counts.audio !== (scenario.endsWith("unload") ? 1 : 0) ||
    counts.pageErrors + counts.captureFailures + counts.drainTimeouts !== 0 ||
    checks.redirected !== (scenario === "redirect" ? 1 : 0) ||
    checks.late !== (scenario.endsWith("unload") ? 1 : 0) ||
    fixture.counts.preparations !== 0 ||
    fixture.counts.redirects !== (scenario === "redirect" ? 1 : 0) ||
    fixture.counts.foreign !== (scenario === "redirect" ? 1 : 0) ||
    fixture.counts.audio !== (scenario === "unrouted-unload" ? 1 : 0) ||
    (scenario === "unrouted-unload"
      ? ![1, 2].includes(checks.outcome)
      : checks.outcome !== 1)
  )
    throw new Error(`Owned network characterization failed; counts=${report}`);
  console.info(`Owned network characterization counts=${report}`);
}

test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});
test.skip(
  process.env.HITO_RECORDING_ATTRIBUTION_E2E !== "1",
  "Requires the opted-in owned recording network characterization",
);

for (const scenario of [
  "preparation",
  "redirect",
  "unrouted-unload",
  "routed-unload",
] as const)
  test(`recording evidence accounts for owned ${scenario} traffic`, async ({
    page,
    context,
  }) => {
    test.setTimeout(15_000);
    page.setDefaultTimeout(2000);
    page.setDefaultNavigationTimeout(2000);
    await characterizeNetwork(page, context, scenario);
  });
