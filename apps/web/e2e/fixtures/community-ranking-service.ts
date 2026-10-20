import { readFile } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import { createServer } from "node:http";
import { isDeepStrictEqual } from "node:util";

const serviceOrigin = "http://127.0.0.1:8095";
const uuid =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

async function boundedJson(response: Response) {
  const parts: Uint8Array[] = [];
  let size = 0;
  const reader = response.body?.getReader();
  if (!reader) throw new Error("Owned service response has no body");
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      if ((size += value.byteLength) > 4096)
        throw new Error("Owned service response exceeded its bound");
      parts.push(value);
    }
  } finally {
    await reader.cancel();
    reader.releaseLock();
  }
  return JSON.parse(Buffer.concat(parts).toString("utf8")) as Record<
    string,
    unknown
  >;
}

export async function rankingService() {
  const admin = (
    await readFile("/run/owned-secrets/community-admin", "utf8")
  ).trimEnd();
  if (!/^[0-9a-f]{64}$/.test(admin))
    throw new Error("Owned service credential differs");
  const privateValues = [admin];
  const receipts: string[] = [];
  const requestIds: string[] = [];
  const active = new Set<Promise<void>>();
  const counters = { rankings: 0, invalid: 0, completed: 0 };
  let candidates: readonly unknown[] | null = null;
  let closing = false;
  async function service(
    path: string,
    value: unknown,
    status: number,
    token?: string,
  ) {
    const response = await fetch(`${serviceOrigin}${path}`, {
      method: "POST",
      signal: AbortSignal.timeout(4000),
      redirect: "error",
      headers: {
        "content-type": "application/json",
        ...(token ? { authorization: `Bearer ${token}` } : {}),
      },
      body: JSON.stringify(value),
    });
    if (
      response.status !== status ||
      response.headers.get("cache-control") !== "no-store" ||
      !uuid.test(response.headers.get("x-request-id") ?? "")
    )
      throw new Error("Owned service protocol status differs");
    return boundedJson(response);
  }
  const server = createServer((request, response) => {
    const upstream = new AbortController();
    response.once("close", () => {
      if (!response.writableFinished) upstream.abort();
    });
    response.once("error", () => {
      if (!closing) counters.invalid++;
    });
    const task = (async () => {
      const permittedHeaders = new Set([
        "host",
        "content-type",
        "content-length",
        "accept",
        "accept-encoding",
        "connection",
        "user-agent",
      ]);
      if (
        !candidates ||
        request.method !== "POST" ||
        request.url !== "/v1/rankings" ||
        Object.keys(request.headers).some(
          (name) => !permittedHeaders.has(name),
        ) ||
        request.headers["content-type"] !== "application/json" ||
        request.headers.accept !== "application/json" ||
        request.headers["accept-encoding"] !== "identity" ||
        request.headers.host !== "127.0.0.1:8096" ||
        !/^Hito\/[0-9A-Za-z.+-]+$/.test(request.headers["user-agent"] ?? "") ||
        ++counters.rankings > 4
      )
        throw new Error("App request differs from the public ranking protocol");
      const chunks: Buffer[] = [];
      let bytes = 0;
      for await (const chunk of request) {
        const part = Buffer.from(chunk);
        if ((bytes += part.length) > 2048)
          throw new Error("App ranking body exceeded its bound");
        chunks.push(part);
      }
      if (request.headers["content-length"] !== String(bytes))
        throw new Error("App ranking framing differs");
      const input: unknown = JSON.parse(Buffer.concat(chunks).toString("utf8"));
      if (!isDeepStrictEqual(input, { candidates }))
        throw new Error("App did not send the exact canonical candidate set");
      const reply = await fetch(`${serviceOrigin}/v1/rankings`, {
        method: "POST",
        body: JSON.stringify(input),
        headers: { "content-type": "application/json" },
        signal: AbortSignal.any([upstream.signal, AbortSignal.timeout(4000)]),
        redirect: "error",
      });
      const requestId = reply.headers.get("x-request-id") ?? "";
      if (
        reply.status !== 200 ||
        reply.headers.get("cache-control") !== "no-store" ||
        !uuid.test(requestId)
      )
        throw new Error(
          "Actual ranking service did not accept the canonical set",
        );
      const result = await boundedJson(reply);
      requestIds.push(requestId);
      const encoded = Buffer.from(JSON.stringify(result));
      response
        .writeHead(200, {
          "content-type": "application/json",
          "content-length": encoded.length,
          "cache-control": "no-store",
          "x-request-id": requestId,
        })
        .end(encoded);
      counters.completed++;
    })()
      .catch(() => {
        counters.invalid++;
        if (!response.headersSent) response.writeHead(502).end();
        else response.destroy();
      })
      .finally(() => active.delete(task));
    active.add(task);
  });
  server.requestTimeout = 4000;
  server.headersTimeout = 4000;
  server.maxConnections = 4;
  server.maxHeadersCount = 32;
  server.setTimeout(4000, (socket) => socket.destroy());
  server.on("clientError", (_error, socket) => {
    if (!closing) counters.invalid++;
    socket.destroy();
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(8096, "127.0.0.1", () => {
      server.off("error", reject);
      resolve();
    });
  });
  server.on("error", () => {
    if (!closing) counters.invalid++;
  });
  return {
    counters,
    privateValues,
    requestIds,
    receipts,
    async seed(exactCandidates: readonly unknown[], preferred: unknown) {
      if (candidates) throw new Error("Owned service may only be seeded once");
      for (let index = 0; index < 2; index++) {
        const reporter = await service("/v1/admin/reporters", {}, 201, admin);
        if (
          Object.keys(reporter).sort().join() !== "reporter_id,token" ||
          typeof reporter.token !== "string" ||
          !/^[0-9a-f]{64}$/.test(reporter.token) ||
          typeof reporter.reporter_id !== "string" ||
          !uuid.test(reporter.reporter_id)
        )
          throw new Error("Owned reporter credential shape differs");
        privateValues.push(reporter.token, reporter.reporter_id);
        const submission = {
          submission_id: randomUUID(),
          source: preferred,
          outcome: "successful_playback",
        };
        const accepted = await service(
          "/v1/outcomes",
          submission,
          201,
          reporter.token,
        );
        if (
          Object.keys(accepted).sort().join() !==
            "receipt_id,status,submission_id" ||
          accepted.submission_id !== submission.submission_id ||
          accepted.status !== "accepted" ||
          typeof accepted.receipt_id !== "string" ||
          !uuid.test(accepted.receipt_id)
        )
          throw new Error(
            "Owned successful outcome did not receive a durable receipt",
          );
        if (
          !isDeepStrictEqual(
            await service("/v1/outcomes", submission, 200, reporter.token),
            accepted,
          )
        )
          throw new Error(
            "Owned outcome replay did not recover the exact receipt",
          );
        receipts.push(accepted.receipt_id);
      }
      const ranked = await service(
        "/v1/rankings",
        { candidates: exactCandidates },
        200,
      );
      const expected = {
        ranked: [
          { source: preferred, successful_reporters: 2 },
          ...exactCandidates
            .filter((value) => !isDeepStrictEqual(value, preferred))
            .map((source) => ({ source, successful_reporters: 0 })),
        ],
      };
      if (!isDeepStrictEqual(ranked, expected))
        throw new Error("Actual service ranking differs after receipt replay");
      candidates = exactCandidates;
    },
    async close() {
      closing = true;
      server.closeAllConnections();
      await new Promise<void>((resolve) => server.close(() => resolve()));
      await Promise.all(active);
      if (active.size)
        throw new Error("Owned ranking callbacks did not retire");
    },
  };
}
