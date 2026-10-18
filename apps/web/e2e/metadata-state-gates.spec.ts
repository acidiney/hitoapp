import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { isDeepStrictEqual } from "node:util";
import {
  expect,
  test,
  type APIRequest,
  type APIRequestContext,
  type APIResponse,
  type Page,
} from "@playwright/test";
import type { components } from "../src/generated";
import { preferencesLibraryState } from "./fixtures/metadata-preferences-state";

type Schema = components["schemas"];
test.use({ trace: "off", screenshot: "off", video: "off" });
test.skip(
  process.env.HITO_METADATA_STATE_GATES_E2E !== "1",
  "Requires explicit metadata state gate opt-in",
);

test("metadata library state rejects oversized progress responses", async ({
  playwright,
}) => {
  await verifyProgressBody(playwright.request, true);
});

test("metadata library state returns and disposes small progress responses", async ({
  playwright,
}) => {
  await verifyProgressBody(playwright.request, false);
});

async function verifyProgressBody(api: APIRequest, oversized: boolean) {
  const saved = {
    ids: ["00000000-0000-4000-8000-000000000001"],
    playlist: "00000000-0000-4000-8000-000000000002" as const,
    entries: [],
    privateValues: [],
  };
  const expected = [
    { recording_id: saved.ids[0], position_ms: 400, speed: 1.25 },
  ] satisfies Schema["RecordingProgress"][];
  const json = JSON.stringify(expected);
  const body = oversized ? json.padEnd(524_289, " ") : json;
  const counts = {
    progress_requests: 0,
    snapshot_reads: 0,
    source_choice_requests: 0,
    unexpected_requests: 0,
    request_errors: 0,
    rejected: 0,
    returned_progress: 0,
    disposed: 0,
    failed: 0,
  };
  const server = createServer(async (request, response) => {
    try {
      const path = new URL(request.url ?? "/", "http://owned.invalid").pathname;
      if (request.method === "POST" && path === "/api/v1/recordings/progress") {
        let bytes = 0;
        const chunks: Buffer[] = [];
        for await (const chunk of request) {
          bytes += chunk.length;
          if (bytes > 4096) throw new Error("Owned request exceeded its bound");
          chunks.push(Buffer.from(chunk));
        }
        if (
          Buffer.concat(chunks).toString() !==
          JSON.stringify({ recording_ids: saved.ids })
        )
          throw new Error("Owned progress request changed");
        counts.progress_requests++;
        response
          .writeHead(200, { "content-type": "application/json" })
          .end(body);
      } else if (
        request.method === "GET" &&
        path === `/api/v1/recordings/${saved.ids[0]}/source-choice`
      ) {
        counts.source_choice_requests++;
        response
          .writeHead(404, { "content-type": "application/json" })
          .end(JSON.stringify({ error: "Resource not found" }));
      } else {
        counts.unexpected_requests++;
        response.writeHead(500).end();
      }
    } catch {
      counts.request_errors++;
      response.writeHead(500).end();
    }
  });
  server.requestTimeout = 10_000;
  server.headersTimeout = 10_000;
  server.setTimeout(10_000, (socket) => socket.destroy());
  let request: APIRequestContext | undefined;
  let progressResponse: APIResponse | undefined;
  try {
    await new Promise<void>((resolve, reject) => {
      server.once("error", reject);
      server.listen(0, "127.0.0.1", () => {
        server.off("error", reject);
        resolve();
      });
    });
    const origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
    request = await api.newContext({ baseURL: origin });
    const post = request.post.bind(request);
    request.post = async (...args) => {
      progressResponse = await post(...args);
      return progressResponse;
    };
    const page = { request, url: () => origin } as Page;
    const unrelated: unknown = [];
    const read = async <T>(): Promise<T> => {
      counts.snapshot_reads++;
      return unrelated as T;
    };
    try {
      const state = await preferencesLibraryState(page, read, saved);
      counts.returned_progress = Number(
        isDeepStrictEqual(state.progress, expected),
      );
    } catch {
      counts.rejected++;
    }
    if (progressResponse) {
      counts.disposed = await progressResponse.body().then(
        () => 0,
        () => 1,
      );
    }
  } catch {
    counts.failed++;
  } finally {
    try {
      await request?.dispose();
    } catch {
      counts.failed++;
    }
    try {
      server.closeAllConnections();
      if (server.listening)
        await new Promise<void>((resolve, reject) =>
          server.close((error) => (error ? reject(error) : resolve())),
        );
    } catch {
      counts.failed++;
    }
  }
  console.log(
    `metadata_state_gate ${JSON.stringify({ oversized: Number(oversized), ...counts })}`,
  );
  expect(
    counts.progress_requests === 1 &&
      counts.snapshot_reads === (oversized ? 0 : 9) &&
      counts.source_choice_requests === Number(!oversized) &&
      counts.unexpected_requests === 0 &&
      counts.request_errors === 0 &&
      counts.rejected === Number(oversized) &&
      counts.returned_progress === Number(!oversized) &&
      counts.disposed === 1 &&
      counts.failed === 0,
  ).toBe(true);
}
