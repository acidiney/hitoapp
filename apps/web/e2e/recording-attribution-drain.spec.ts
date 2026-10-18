import { randomUUID } from "node:crypto";
import { createServer, type ServerResponse } from "node:http";
import type { AddressInfo } from "node:net";
import { expect, test } from "@playwright/test";
import { recordingAttributionEvidence } from "./fixtures/recording-attribution-evidence";

async function within<T>(work: Promise<T>, milliseconds: number): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      work,
      new Promise<never>((_resolve, reject) => {
        timer = setTimeout(
          () => reject(new Error("Owned recording drain deadline exceeded")),
          milliseconds,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

async function heldRecordingStatus(account: string, recording: string) {
  const path = `/api/v1/recordings/${recording}/download`;
  const body = '{"error":"Resource not found"}';
  const counts = { requests: 0, held: 0, unexpected: 0, released: 0 };
  let response: ServerResponse | null = null;
  let closing = false;
  const server = createServer((request, reply) => {
    reply.on("error", () => {
      if (!closing) counts.unexpected++;
    });
    if (++counts.requests > 4) {
      counts.unexpected++;
      reply.writeHead(429).end();
    } else if (request.method === "GET" && request.url === "/") {
      reply
        .writeHead(200, { "content-type": "text/html" })
        .end("<!doctype html><title>Owned recording drain fixture</title>");
    } else if (request.method === "GET" && request.url === "/favicon.ico") {
      reply.writeHead(204).end();
    } else if (
      request.method === "GET" &&
      request.url === path &&
      request.headers["x-hito-request"] === "1" &&
      request.headers["x-hito-account"] === account &&
      counts.held === 0
    ) {
      counts.held++;
      response = reply;
      reply.writeHead(404, {
        "content-type": "application/json",
        "content-length": Buffer.byteLength(body),
      });
      reply.write(body.slice(0, 10));
    } else {
      counts.unexpected++;
      reply.writeHead(400).end();
    }
  });
  server.requestTimeout = 4000;
  server.headersTimeout = 4000;
  server.maxHeadersCount = 32;
  server.maxConnections = 4;
  server.setTimeout(12_000, (socket) => socket.destroy());
  server.on("clientError", (_error, socket) => {
    if (!closing) counts.unexpected++;
    socket.destroy();
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      resolve();
    });
  });
  server.on("error", () => {
    if (!closing) counts.unexpected++;
  });
  return {
    origin: `http://127.0.0.1:${(server.address() as AddressInfo).port}`,
    path,
    counts,
    release() {
      if (response && !response.writableEnded && !response.destroyed) {
        counts.released++;
        response.end(body.slice(10));
      }
    },
    async close() {
      closing = true;
      const stopped = new Promise<void>((resolve, reject) => {
        server.close((error) =>
          error
            ? reject(new Error("Owned recording drain fixture close failed"))
            : resolve(),
        );
      });
      server.closeAllConnections();
      await within(stopped, 1000);
    },
  };
}

test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});
test.skip(
  process.env.HITO_RECORDING_ATTRIBUTION_E2E !== "1",
  "Requires the opted-in owned recording evidence regression",
);

test("recording evidence rejects a held status body within its drain deadline and independently cleans up", async ({
  page,
  context,
}) => {
  test.setTimeout(20_000);
  page.setDefaultTimeout(2000);
  page.setDefaultNavigationTimeout(2000);
  const account = randomUUID();
  const recording = randomUUID();
  const checks = {
    drainOutcome: 0,
    drainMilliseconds: 0,
    browserCloseAttempted: 0,
    browserClosed: 0,
    providerCloseAttempted: 0,
    providerClosed: 0,
    drainSettledAfterRelease: 0,
    drainTimeouts: 0,
    stickyDrainOutcome: 0,
    setupFailed: 0,
  };
  let fixture: Awaited<ReturnType<typeof heldRecordingStatus>> | null = null;
  let evidence: Awaited<
    ReturnType<typeof recordingAttributionEvidence>
  > | null = null;
  let drain: Promise<number> | null = null;
  try {
    expect(process.env.PLAYWRIGHT_NO_COPY_PROMPT === "1").toBe(true);
    fixture = await heldRecordingStatus(account, recording);
    evidence = await recordingAttributionEvidence(context, fixture.origin, []);
    evidence.watchRecording(account, recording);
    await page.goto(fixture.origin);
    const received = page.waitForResponse(
      (reply) => new URL(reply.url()).pathname === fixture!.path,
      { timeout: 2000 },
    );
    const fetching = page.evaluate(
      ({ path, account }) => {
        const result = globalThis as unknown as Record<string, unknown>;
        result.ownedHeldStatusOutcome = 0;
        void fetch(path, {
          headers: { "x-hito-request": "1", "x-hito-account": account },
        }).then(
          async (reply) => {
            try {
              await reply.text();
              result.ownedHeldStatusOutcome = 1;
            } catch {
              result.ownedHeldStatusOutcome = 2;
            }
          },
          () => {
            result.ownedHeldStatusOutcome = 3;
          },
        );
      },
      { path: fixture.path, account },
    );
    await Promise.all([received, fetching]);
    expect(
      fixture.counts.held === 1 &&
        evidence.counts.statusReads === 1 &&
        (await page.evaluate(
          () =>
            (globalThis as unknown as Record<string, unknown>)
              .ownedHeldStatusOutcome,
        )) === 0,
    ).toBe(true);
    const started = Date.now();
    drain = evidence.finish().then(
      () => 1,
      () => 2,
    );
    try {
      checks.drainOutcome = await within(drain, 6000);
    } catch {
      checks.drainOutcome = 3;
    }
    checks.drainMilliseconds = Date.now() - started;
  } catch {
    checks.setupFailed = 1;
  } finally {
    fixture?.release();
    checks.browserCloseAttempted = 1;
    try {
      await within(context.close(), 1500);
      checks.browserClosed = 1;
    } catch {
      checks.browserClosed = 0;
    }
    checks.providerCloseAttempted = 1;
    try {
      await fixture?.close();
      checks.providerClosed = fixture ? 1 : 0;
    } catch {
      checks.providerClosed = 0;
    }
    if (drain) {
      try {
        await within(drain, 1000);
        checks.drainSettledAfterRelease = 1;
      } catch {
        checks.drainSettledAfterRelease = 0;
      }
    }
    checks.drainTimeouts = evidence?.counts.drainTimeouts ?? 0;
    if (evidence) {
      try {
        checks.stickyDrainOutcome = await within(
          evidence.finish().then(
            () => 1,
            () => 2,
          ),
          1000,
        );
      } catch {
        checks.stickyDrainOutcome = 3;
      }
    }
  }
  const report = JSON.stringify({ ...checks, ...fixture?.counts });
  if (Buffer.byteLength(report) > 3800)
    throw new Error(
      "Owned recording drain regression report exceeded its bound",
    );
  if (
    checks.setupFailed ||
    checks.drainOutcome !== 2 ||
    checks.drainMilliseconds > 6000 ||
    checks.browserClosed !== 1 ||
    checks.providerClosed !== 1 ||
    checks.drainSettledAfterRelease !== 1 ||
    checks.drainTimeouts !== 1 ||
    checks.stickyDrainOutcome !== 2 ||
    !fixture ||
    fixture.counts.unexpected !== 0 ||
    fixture.counts.released !== 1
  )
    throw new Error(
      `Recording attribution drain regression failed; counts=${report}`,
    );
  console.info(`Recording attribution drain counts=${report}`);
});
