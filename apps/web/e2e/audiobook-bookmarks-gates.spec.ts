import { randomUUID } from "node:crypto";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { expect, test, type Request, type Route } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import { bookmarkDelivery } from "./fixtures/audiobook-bookmarks-delivery";
import { bookmarkEvidence } from "./fixtures/audiobook-bookmarks-evidence";

test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});
test.skip(
  process.env.HITO_AUDIOBOOK_BOOKMARKS_E2E !== "1",
  "Requires explicit bookmark browser gate opt-in",
);

test("preparation blocks foreign audio-endpoint and download traffic while recording failed and rejected API responses", async ({
  page,
  context,
}) => {
  test.setTimeout(30_000);
  const origin = "https://owned-bookmark.invalid";
  const evidence = await bookmarkEvidence(context, origin, []);
  const pending = new Set<Promise<void>>();
  const counts = {
    owned_documents: 0,
    blocked: 0,
    forbidden_delivered: 0,
    forced_failures: 0,
    rejected_replies: 0,
    unexpected: 0,
    audio_endpoint_settled: false,
  };
  const diagnostics = {
    traffic_started: 0,
    traffic_get: 0,
    traffic_options: 0,
    traffic_other_method: 0,
    traffic_media: 0,
    traffic_fetch: 0,
    traffic_other_type: 0,
    fixture_requests: 0,
    fixture_get: 0,
    fixture_options: 0,
    fixture_media: 0,
    fixture_fetch: 0,
    fixture_other_type: 0,
    unexpected_fixture: 0,
    unexpected_options: 0,
    route_failures: 0,
  };
  const classify = (request: Request) => {
    const fixture =
      new URL(request.url()).pathname === "/audio/owned-fixture.wav";
    const method = request.method();
    const resource = request.resourceType();
    diagnostics.traffic_started++;
    diagnostics.traffic_get += Number(method === "GET");
    diagnostics.traffic_options += Number(method === "OPTIONS");
    diagnostics.traffic_other_method += Number(
      !["GET", "OPTIONS"].includes(method),
    );
    diagnostics.traffic_media += Number(resource === "media");
    diagnostics.traffic_fetch += Number(resource === "fetch");
    diagnostics.traffic_other_type += Number(
      !["media", "fetch"].includes(resource),
    );
    if (fixture) {
      diagnostics.fixture_requests++;
      diagnostics.fixture_get += Number(method === "GET");
      diagnostics.fixture_options += Number(method === "OPTIONS");
      diagnostics.fixture_media += Number(resource === "media");
      diagnostics.fixture_fetch += Number(resource === "fetch");
      diagnostics.fixture_other_type += Number(
        !["media", "fetch"].includes(resource),
      );
    }
  };
  context.on("request", classify);
  let failed = false;
  const handler = async (route: Route) => {
    const running = (async () => {
      const request = route.request();
      const url = new URL(request.url());
      if (!evidence.allowed(request)) {
        counts.blocked++;
        await route.abort("blockedbyclient");
      } else if (
        url.origin === origin &&
        url.pathname === "/" &&
        request.isNavigationRequest() &&
        request.method() === "GET"
      ) {
        counts.owned_documents++;
        await route.fulfill({
          status: 200,
          contentType: "text/html",
          body: '<!doctype html><html><head><link rel="icon" href="data:,"></head><body></body></html>',
        });
      } else if (
        url.hostname === "foreign-bookmark.invalid" ||
        url.pathname === "/audio/owned-fixture.wav" ||
        request.resourceType() === "media" ||
        url.pathname === "/download/owned.bin"
      ) {
        counts.forbidden_delivered++;
        await route.fulfill({
          status: 204,
          headers: { "access-control-allow-origin": "*" },
        });
      } else if (url.pathname === "/api/v1/preparation-failure") {
        counts.forced_failures++;
        await route.abort("failed");
      } else if (url.pathname === "/api/v1/preparation-status") {
        counts.rejected_replies++;
        await route.fulfill({
          status: 503,
          contentType: "application/json",
          headers: { "access-control-allow-origin": "*" },
          body: '{"error":"Owned rejection"}',
        });
      } else {
        counts.unexpected++;
        diagnostics.unexpected_fixture += Number(
          url.pathname === "/audio/owned-fixture.wav",
        );
        diagnostics.unexpected_options += Number(
          request.method() === "OPTIONS",
        );
        await route.fulfill({
          status: 204,
          headers: { "access-control-allow-origin": "*" },
        });
      }
    })()
      .catch(() => {
        counts.unexpected++;
        diagnostics.route_failures++;
      })
      .finally(() => pending.delete(running));
    pending.add(running);
    await running;
  };
  try {
    await context.route("**/*", handler);
    await page.goto(origin, { timeout: 10_000 });
    counts.audio_endpoint_settled = await page.evaluate(async (ownedOrigin) => {
      const read = async (url: string) => {
        const controller = new AbortController();
        const deadline = setTimeout(() => controller.abort(), 3000);
        try {
          const response = await fetch(url, { signal: controller.signal });
          const body = await response.arrayBuffer();
          if (body.byteLength > 4096) return -1;
          return response.status;
        } catch {
          return 0;
        } finally {
          clearTimeout(deadline);
        }
      };
      const results = await Promise.all([
        read("https://foreign-bookmark.invalid/owned"),
        read(`${ownedOrigin}/download/owned.bin`),
        read(`${ownedOrigin}/api/v1/preparation-failure`),
        read(`${ownedOrigin}/api/v1/preparation-status`),
        read(`${ownedOrigin}/audio/owned-fixture.wav`),
      ]);
      return results.every(
        (status, index) => status === (index === 3 ? 503 : 0),
      );
    }, origin);
  } catch {
    failed = true;
  } finally {
    try {
      await context.unroute("**/*", handler);
      while (pending.size) await Promise.allSettled([...pending]);
    } catch {
      failed = true;
    }
    try {
      await context.close();
    } catch {
      failed = true;
    }
    try {
      await evidence.finish();
    } catch {
      failed = true;
    }
    context.off("request", classify);
  }
  const observed: Record<string, number> = evidence.counts;
  const verified =
    counts.owned_documents === 1 &&
    counts.blocked === 3 &&
    counts.forbidden_delivered === 0 &&
    counts.forced_failures === 1 &&
    counts.rejected_replies === 1 &&
    counts.unexpected === 0 &&
    counts.audio_endpoint_settled &&
    observed.foreign === 1 &&
    observed.audio === 1 &&
    observed.downloads === 1 &&
    observed.workflow_requests === 0 &&
    observed.preparation_failures === 4 &&
    observed.preparation_status_errors === 1;
  safeReport("preparation", failed || !verified, {
    ...counts,
    ...diagnostics,
    ...evidence.counts,
  });
});

test("retiring delivery during an owned response fetch cannot create a late confirmation gate", async ({
  page,
  context,
}) => {
  test.setTimeout(30_000);
  const account = randomUUID();
  const book = randomUUID();
  const entry = randomUUID();
  const recording = randomUUID();
  const label = "Owned private bookmark gate fixture";
  const detail = {
    playlist: { id: book, name: "Owned gate audiobook", kind: "local" },
    chapters: [
      {
        entry_id: entry,
        recording_id: recording,
        position: 0,
        title: "Owned gate chapter",
        inherited_title: "Owned gate chapter",
        artist: "Owned narrator",
        inherited_artist: "Owned narrator",
        duration_ms: 60_000,
        liked: false,
        available: true,
      },
    ],
    bookmarks: [],
  } satisfies Schema<"AudiobookDetail">;
  const batch = {
    device_id: randomUUID(),
    operations: [
      {
        id: randomUUID(),
        action: {
          kind: "audiobook_bookmark_create",
          audiobook_id: book,
          bookmark_id: randomUUID(),
          entry_id: entry,
          recording_id: recording,
          position_ms: 35_500,
          label,
        },
      },
    ],
  } satisfies Schema<"SyncBatch">;
  const server = await ownedResponder(detail, batch);
  const evidence = await bookmarkEvidence(context, server.origin, [label]);
  const delivery = await bookmarkDelivery(context, server.origin, evidence);
  const counts = {
    accepted_posts: 0,
    detail_received: false,
    close_settled: false,
    late_gates: 0,
    cleanup_settled: false,
  };
  let failed = false;
  let reading: Promise<number> | null = null;
  let closing: Promise<void> | null = null;
  try {
    await page.goto(server.origin);
    await page.waitForLoadState("networkidle");
    delivery.arm(account, detail, detail.chapters[0], label);
    evidence.arm();
    const request = { account, text: JSON.stringify(batch) };
    const first = await page.evaluate(async ({ account, text }) => {
      try {
        const response = await fetch("/api/v1/sync", {
          method: "POST",
          headers: {
            "content-type": "application/json",
            "x-hito-account": account,
            "x-hito-request": "1",
          },
          body: text,
        });
        await response.arrayBuffer();
        return response.status;
      } catch {
        return 0;
      }
    }, request);
    const replay = await page.evaluate(async ({ account, text }) => {
      try {
        const response = await fetch("/api/v1/sync", {
          method: "POST",
          headers: {
            "content-type": "application/json",
            "x-hito-account": account,
            "x-hito-request": "1",
          },
          body: text,
        });
        await response.arrayBuffer();
        return response.status;
      } catch {
        return 0;
      }
    }, request);
    expect(
      first === 0 &&
        replay === 200 &&
        delivery.counts.create === 1 &&
        delivery.counts.replay === 1,
    ).toBe(true);
    counts.accepted_posts = server.counts.posts;
    reading = page
      .evaluate(
        async ({ account, book }) => {
          try {
            const response = await fetch(`/api/v1/audiobooks/${book}`, {
              headers: { "x-hito-account": account, "x-hito-request": "1" },
            });
            await response.arrayBuffer();
            return response.status;
          } catch {
            return 0;
          }
        },
        { account, book },
      )
      .catch(() => 0);
    counts.detail_received = await settles(server.detailStarted, 3000);
    expect(counts.detail_received).toBe(true);
    closing = delivery.close();
    server.release();
    counts.close_settled = await settles(closing, 1500);
    counts.late_gates = delivery.counts.held_reads;
    expect(
      counts.close_settled &&
        counts.late_gates === 0 &&
        delivery.counts.invalid === 0,
    ).toBe(true);
  } catch {
    failed = true;
  } finally {
    server.release();
    for (let number = 1; number <= delivery.counts.held_reads; number++) {
      try {
        delivery.release(number);
      } catch {}
    }
    if (!closing) closing = delivery.close();
    counts.cleanup_settled = await settles(closing, 4000);
    try {
      await context.close();
    } catch {
      failed = true;
    }
    if (reading && !(await settles(reading, 4000))) failed = true;
    try {
      await evidence.finish();
    } catch {
      failed = true;
    }
    try {
      await server.close();
    } catch {
      failed = true;
    }
  }
  safeReport(
    "retired-delivery",
    failed || !counts.cleanup_settled || server.counts.errors !== 0,
    {
      ...counts,
      delivery_invalid: delivery.counts.invalid,
      server_errors: server.counts.errors,
    },
  );
});

async function ownedResponder(
  detail: Schema<"AudiobookDetail">,
  batch: Schema<"SyncBatch">,
) {
  const counts = { posts: 0, errors: 0, requests: 0 };
  let release!: () => void;
  let started!: () => void;
  const reply = new Promise<void>((resolve) => {
    release = resolve;
  });
  const detailStarted = new Promise<void>((resolve) => {
    started = resolve;
  });
  const action = batch.operations[0].action;
  if (action.kind !== "audiobook_bookmark_create")
    throw new Error("The owned gate needs one create action");
  const server = createServer(async (request, response) => {
    try {
      if (++counts.requests > 10)
        throw new Error("Owned gate request budget exceeded");
      const path = new URL(request.url ?? "/", "http://owned.invalid").pathname;
      if (request.method === "GET" && path === "/") {
        response
          .writeHead(200, { "content-type": "text/html" })
          .end(
            '<!doctype html><html><head><link rel="icon" href="data:,"></head><body></body></html>',
          );
        return;
      }
      const chunks: Buffer[] = [];
      let bytes = 0;
      for await (const chunk of request) {
        bytes += chunk.length;
        if (bytes > 4096)
          throw new Error("Owned gate request body exceeded its bound");
        chunks.push(Buffer.from(chunk));
      }
      let body: unknown;
      if (request.method === "POST" && path === "/api/v1/sync") {
        if (Buffer.concat(chunks).toString() !== JSON.stringify(batch))
          throw new Error("Owned gate mutation changed");
        counts.posts++;
        body = {
          acknowledged: [batch.operations[0].id],
        } satisfies Schema<"SyncReceipt">;
      } else if (
        request.method === "GET" &&
        path === `/api/v1/audiobooks/${detail.playlist.id}` &&
        bytes === 0
      ) {
        started();
        await reply;
        body = {
          ...detail,
          bookmarks: [
            {
              id: action.bookmark_id,
              audiobook_id: action.audiobook_id,
              entry_id: action.entry_id,
              recording_id: action.recording_id,
              position_ms: action.position_ms,
              label: action.label,
            } satisfies Schema<"AudiobookBookmark">,
          ],
        } satisfies Schema<"AudiobookDetail">;
      } else throw new Error("Owned gate route is unavailable");
      response
        .writeHead(200, {
          "content-type": "application/json",
          "cache-control": "private, no-store",
        })
        .end(JSON.stringify(body));
    } catch {
      counts.errors++;
      response.writeHead(500).end();
    }
  });
  server.requestTimeout = 10_000;
  server.headersTimeout = 10_000;
  server.setTimeout(10_000, (socket) => socket.destroy());
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      resolve();
    });
  });
  return {
    origin: `http://127.0.0.1:${(server.address() as AddressInfo).port}`,
    counts,
    detailStarted,
    release,
    async close() {
      release();
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    },
  };
}

async function settles(promise: Promise<unknown>, milliseconds: number) {
  let timeout!: ReturnType<typeof setTimeout>;
  try {
    return await Promise.race([
      promise.then(
        () => true,
        () => false,
      ),
      new Promise<boolean>((resolve) => {
        timeout = setTimeout(() => resolve(false), milliseconds);
      }),
    ]);
  } finally {
    clearTimeout(timeout);
  }
}

function safeReport(
  gate: "preparation" | "retired-delivery",
  failed: boolean,
  counts: Record<string, boolean | number>,
) {
  const report = JSON.stringify({ gate, failed, counts });
  if (Buffer.byteLength(report) > 4096)
    throw new Error("The bookmark gate evidence exceeded its output bound");
  console.log(`audiobook_bookmarks_gate ${report}`);
  if (failed) throw new Error(`The bookmark ${gate} gate verification failed`);
}
