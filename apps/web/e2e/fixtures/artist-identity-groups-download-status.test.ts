import { randomUUID } from "node:crypto";
import { createServer, request as httpRequest } from "node:http";
import type { AddressInfo } from "node:net";
import { expect, it } from "vitest";
import { artistGroupDownloadStatus } from "./artist-identity-groups-download-status";

type Mode =
  | "playlist"
  | "recording"
  | "foreign_recording"
  | "wrong_account"
  | "post"
  | "noncanonical"
  | "private_header_missing"
  | "unregistered"
  | "unconsumed"
  | "query"
  | "wrong_request_header";

async function exercise(mode: Mode) {
  const account = randomUUID();
  const recordings = Array.from({ length: 4 }, () => randomUUID());
  const playlist = randomUUID();
  const counts = {
    download: 0,
    downloadStatusReads: 0,
    downloadStatusAbsentReplies: 0,
    invalidDownloadStatus: 0,
    bounds: 0,
  };
  const canonical = JSON.stringify({ error: "Resource not found" });
  const responseBody =
    mode === "noncanonical"
      ? JSON.stringify({ error: "Different response" })
      : canonical;
  const observed = { requests: 0, methodIsGet: false, bodyBytes: 0 };
  const server = createServer((request, response) => {
    observed.requests++;
    observed.methodIsGet = request.method === "GET";
    request.on("data", (chunk: Buffer) => {
      observed.bodyBytes += chunk.length;
      if (observed.bodyBytes > 32) request.destroy();
    });
    request.on("end", () => {
      response
        .writeHead(404, {
          "content-type": "application/json",
          "content-length": Buffer.byteLength(responseBody),
          ...(mode === "private_header_missing"
            ? {}
            : { "cache-control": "private, no-store" }),
        })
        .end(responseBody);
    });
  });
  server.requestTimeout = 2500;
  server.headersTimeout = 2500;
  server.setTimeout(2500, (socket) => socket.destroy());
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      resolve();
    });
  });
  const origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  const boundary = artistGroupDownloadStatus(origin, counts);
  const recording = ["recording", "foreign_recording"].includes(mode);
  const id =
    mode === "foreign_recording"
      ? randomUUID()
      : recording
        ? recordings[0]
        : playlist;
  const path = `/api/v1/${recording ? "recordings" : "playlists"}/${id}/download${mode === "query" ? "?unexpected=1" : ""}`;
  const method = mode === "post" ? "POST" : "GET";
  const body = mode === "post" ? "{}" : null;
  const headers = {
    "x-hito-request": mode === "wrong_request_header" ? "0" : "1",
    "x-hito-account": mode === "wrong_account" ? randomUUID() : account,
    ...(body === null
      ? {}
      : { "content-length": String(Buffer.byteLength(body)) }),
  };
  const ticket = boundary.request({
    url: `${origin}${path}`,
    method,
    body,
    headers,
  });
  let wireStatus = 0;
  let wireBytes = 0;
  try {
    const received = await new Promise<{
      status: number;
      headers: Record<string, string>;
      bytes: Uint8Array;
    }>((resolve, reject) => {
      const request = httpRequest(
        `${origin}${path}`,
        { method, headers, agent: false },
        (response) => {
          const chunks: Buffer[] = [];
          response.on("data", (chunk: Buffer) => {
            wireBytes += chunk.length;
            if (wireBytes > 4096) {
              response.destroy();
              reject(
                new Error("Owned status control exceeded its response bound"),
              );
            } else chunks.push(chunk);
          });
          response.once("error", () =>
            reject(new Error("Owned status control response failed")),
          );
          response.once("end", () =>
            resolve({
              status: response.statusCode ?? 0,
              headers: Object.fromEntries(
                Object.entries(response.headers).filter(
                  (entry): entry is [string, string] =>
                    typeof entry[1] === "string",
                ),
              ),
              bytes: Buffer.concat(chunks),
            }),
          );
        },
      );
      request.setTimeout(2500, () =>
        request.destroy(new Error("Owned status control timed out")),
      );
      request.once("error", () =>
        reject(new Error("Owned status control request failed")),
      );
      request.end(body);
    });
    wireStatus = received.status;
    if (ticket !== null && mode !== "unconsumed")
      await boundary.reply(ticket, {
        status: received.status,
        headers: received.headers,
        body: async () => received.bytes,
      });
    const beforeWatch = { ...counts };
    if (mode !== "unregistered") boundary.watch(account, recordings, playlist);
    boundary.finalize();
    const once = { ...counts };
    boundary.finalize();
    expect(counts).toEqual(once);
    expect(
      observed.requests === 1 &&
        wireStatus === 404 &&
        wireBytes === Buffer.byteLength(responseBody) &&
        observed.methodIsGet === (method === "GET") &&
        observed.bodyBytes === (body === null ? 0 : 2),
    ).toBe(true);
    return {
      counts,
      beforeWatch,
      canonicalBytes: Buffer.byteLength(canonical),
    };
  } finally {
    server.closeAllConnections();
    await new Promise<void>((resolve, reject) => {
      const timer = setTimeout(
        () => reject(new Error("Owned status control close timed out")),
        1000,
      );
      server.close((error) => {
        clearTimeout(timer);
        if (error) reject(new Error("Owned status control close failed"));
        else resolve();
      });
    });
  }
}

it.each(["playlist", "recording"] as const)(
  "authorizes a late exact owned %s status and canonical reply",
  async (mode) => {
    const { counts, beforeWatch, canonicalBytes } = await exercise(mode);
    expect(canonicalBytes).toBe(30);
    expect(beforeWatch).toEqual({
      download: 0,
      downloadStatusReads: 0,
      downloadStatusAbsentReplies: 0,
      invalidDownloadStatus: 0,
      bounds: 0,
    });
    expect(counts).toEqual({
      download: 0,
      downloadStatusReads: 1,
      downloadStatusAbsentReplies: 1,
      invalidDownloadStatus: 0,
      bounds: 0,
    });
  },
);

it.each([
  ["foreign_recording", 1, 0],
  ["wrong_account", 1, 0],
  ["post", 1, 0],
  ["noncanonical", 0, 1],
  ["private_header_missing", 0, 1],
  ["unregistered", 0, 1],
  ["unconsumed", 0, 1],
  ["query", 1, 0],
  ["wrong_request_header", 1, 0],
] as const)(
  "rejects the actual owned HTTP control %s",
  async (mode, download, invalid) => {
    const { counts } = await exercise(mode);
    expect(counts.download).toBe(download);
    expect(counts.invalidDownloadStatus).toBe(invalid);
    expect(counts.downloadStatusAbsentReplies).toBe(0);
    expect(counts.bounds).toBe(0);
  },
);
