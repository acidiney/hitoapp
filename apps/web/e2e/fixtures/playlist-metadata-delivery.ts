import type {
  BrowserContext,
  Page,
  Request as BrowserRequest,
} from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import type { playlistMetadataProvider } from "./playlist-metadata-provider";

export async function metadataDelivery(
  context: BrowserContext,
  page: Page,
  origin: string,
  owned: Awaited<ReturnType<typeof playlistMetadataProvider>>,
) {
  let path: string | null = null;
  let playlist: string | null = null;
  let lostRequest: BrowserRequest | null = null;
  const evidence = {
    bodies: [] as string[],
    receipt: null as Buffer | null,
    failed: false,
    traffic: {
      total: 0,
      foreign: 0,
      audio: 0,
      download: 0,
      source: 0,
      adoption: 0,
      settings: 0,
      pageErrors: 0,
      lostFailures: 0,
      unexpectedFailures: 0,
    },
  };
  const { traffic, bodies } = evidence;
  page.on("pageerror", () => traffic.pageErrors++);
  page.on("requestfailed", (request) => {
    if (new URL(request.url()).pathname !== path) return;
    if (request === lostRequest) traffic.lostFailures++;
    else traffic.unexpectedFailures++;
  });
  await context.route("**/*", async (route) => {
    const request = route.request();
    const target = new URL(request.url());
    traffic.total++;
    const foreign = target.origin !== origin;
    const audio =
      request.resourceType() === "media" || /\/audio$/.test(target.pathname);
    const writing = !["GET", "HEAD", "OPTIONS"].includes(request.method());
    const download =
      writing && /\/(?:download|downloads)(?:\/|$)/.test(target.pathname);
    const source =
      writing &&
      /\/recordings\/[^/]+\/(?:sources|source-choice)(?:\/|$)/.test(
        target.pathname,
      );
    const adoption =
      writing && /\/plugins\/[^/]+\/recordings$/.test(target.pathname);
    if (foreign) traffic.foreign++;
    if (audio) traffic.audio++;
    if (download) traffic.download++;
    if (source) traffic.source++;
    if (adoption) traffic.adoption++;
    if (target.pathname === "/api/v1/plugins" && request.method() === "GET")
      traffic.settings++;
    if (
      foreign ||
      audio ||
      download ||
      source ||
      adoption ||
      traffic.total > 400
    ) {
      await route.abort();
      return;
    }
    if (target.pathname !== path || request.method() !== "POST") {
      await route.continue();
      return;
    }
    const body = request.postData();
    if (body === null || Buffer.byteLength(body) > 2048 || bodies.length >= 2) {
      evidence.failed = true;
      await route.abort();
      return;
    }
    bodies.push(body);
    if (lostRequest !== null) {
      await route.continue();
      return;
    }
    lostRequest = request;
    let response: Awaited<ReturnType<typeof route.fetch>> | null = null;
    try {
      response = await route.fetch({ timeout: 10_000, maxRetries: 0 });
      if (response.status() !== 200)
        throw new Error("The original metadata request was not accepted");
      const bytes = await response.body();
      if (bytes.length > 8192)
        throw new Error("The metadata receipt exceeded its bound");
      const receipt = JSON.parse(
        bytes.toString(),
      ) as Schema<"ImportedPlaylistMetadataRefresh">;
      const sent = JSON.parse(
        body,
      ) as Schema<"RefreshImportedPlaylistMetadata">;
      if (
        receipt.request_id !== sent.request_id ||
        receipt.playlist_id !== playlist ||
        receipt.updated_recordings !== 2 ||
        receipt.unchanged_recordings !== 0 ||
        receipt.unavailable_recordings !== 0
      )
        throw new Error(
          "The original metadata receipt did not match the request",
        );
      evidence.receipt = bytes;
      owned.offline();
    } catch {
      evidence.failed = true;
    } finally {
      try {
        if (response) await response.dispose();
      } catch {
        evidence.failed = true;
      }
      try {
        await route.abort("failed");
      } catch {
        evidence.failed = true;
      }
    }
  });
  return {
    evidence,
    observe: (id: string) => {
      if (playlist !== null)
        throw new Error("A metadata delivery observer already owns a playlist");
      playlist = id;
      path = `/api/v1/playlists/${id}/metadata/refresh`;
      return path;
    },
  };
}
