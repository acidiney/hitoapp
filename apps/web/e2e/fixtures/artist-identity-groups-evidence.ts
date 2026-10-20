import { writeFile } from "node:fs/promises";
import type { BrowserContext, Request } from "@playwright/test";
import {
  artistGroupsConsole,
  type ArtistGroupCommands,
} from "./artist-identity-groups-console";
import { artistGroupDownloadStatus } from "./artist-identity-groups-download-status";

const uuid = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;

export async function artistGroupsEvidence(
  context: BrowserContext,
  origin: string,
  privateValues: string[],
) {
  const pending = new Set<Promise<void>>();
  const statusRequests = new WeakMap<Request, number>();
  const commands: ArtistGroupCommands = new Map();
  let target: { account: string; ids: Set<string> } | null = null;
  let armed = false;
  const counts = {
    requests: 0,
    foreign: 0,
    audio: 0,
    download: 0,
    sourceWrites: 0,
    unexpectedWrites: 0,
    downloadStatusReads: 0,
    downloadStatusAbsentReplies: 0,
    invalidDownloadStatus: 0,
    artworkWrites: 0,
    chooseRequests: 0,
    resetRequests: 0,
    acceptedChoices: 0,
    failedChoices: 0,
    acceptedResets: 0,
    acceptedReads: 0,
    reviewStarts: 0,
    reviewFinishes: 0,
    pageErrors: 0,
    malformedLogs: 0,
    privateConsole: 0,
    consoleBytes: 0,
    bounds: 0,
    drainTimeout: 0,
  };
  const downloadStatus = artistGroupDownloadStatus(origin, counts);
  const pageError = () => counts.pageErrors++;
  context.on("page", (page) => page.on("pageerror", pageError));
  for (const page of context.pages()) page.on("pageerror", pageError);
  context.on("request", (request) => {
    if (++counts.requests > 500) counts.bounds++;
    const url = new URL(request.url());
    const method = request.method();
    const path = url.pathname;
    if (["http:", "https:"].includes(url.protocol) && url.origin !== origin)
      counts.foreign++;
    if (request.resourceType() === "media" || /\/audio(?:\/|$)/.test(path))
      counts.audio++;
    const ticket = downloadStatus.request({
      url: request.url(),
      method,
      body: request.postData(),
      headers: request.headers(),
    });
    if (ticket !== null) statusRequests.set(request, ticket);
    const writing = !["GET", "HEAD", "OPTIONS"].includes(method);
    if (
      writing &&
      /\/(?:sources|source-choice|preparations|resolution)(?:\/|$)/.test(path)
    )
      counts.sourceWrites++;
    if (!armed || !writing) return;
    const recording =
      /^\/api\/v1\/recordings\/([^/]+)\/(artist-identity(?:\/reset)?|artist-review\/context)$/.exec(
        path,
      );
    if (
      url.origin !== origin ||
      url.search ||
      !target ||
      request.headers()["x-hito-request"] !== "1" ||
      request.headers()["x-hito-account"] !== target.account
    ) {
      counts.unexpectedWrites++;
      return;
    }
    if (recording && target.ids.has(recording[1])) {
      if (recording[2] === "artist-review/context" && method === "POST") return;
      if (
        (recording[2] === "artist-identity" && method === "PUT") ||
        (recording[2] === "artist-identity/reset" && method === "POST")
      ) {
        try {
          const raw = request.postData();
          if (!raw || Buffer.byteLength(raw) > 16_384)
            throw new Error("Invalid owned command envelope");
          const value = JSON.parse(raw) as Record<string, unknown>;
          const action = method === "PUT" ? "choose" : "reset";
          if (
            typeof value.request_id !== "string" ||
            !uuid.test(value.request_id)
          )
            throw new Error("Invalid owned command correlation");
          const known = commands.get(value.request_id);
          if (known && known.action !== action)
            throw new Error("Conflicting owned command correlation");
          if (!known)
            commands.set(value.request_id, { action, accepted: 0, failed: 0 });
          if (action === "choose") counts.chooseRequests++;
          else counts.resetRequests++;
          return;
        } catch {
          counts.unexpectedWrites++;
          return;
        }
      }
    }
    if (path === "/api/v1/recordings/progress" && method === "POST") return;
    if (
      /^\/api\/v1\/(recordings|releases|playlists)\/[^/]+\/artwork\/fetch$/.test(
        path,
      ) &&
      method === "POST"
    ) {
      counts.artworkWrites++;
      return;
    }
    counts.unexpectedWrites++;
  });
  context.on("response", (response) => {
    const ticket = statusRequests.get(response.request());
    if (ticket === undefined) return;
    if (pending.size >= 32) {
      counts.bounds++;
      return;
    }
    const checking = downloadStatus
      .reply(ticket, {
        status: response.status(),
        headers: response.headers(),
        body: () => response.body(),
      })
      .finally(() => pending.delete(checking));
    pending.add(checking);
  });
  const workflowConsole = await artistGroupsConsole(
    context,
    origin,
    privateValues,
    pending,
    counts,
    commands,
  );
  return {
    counts,
    watch(account: string, ids: string[], playlist: string) {
      if (
        target ||
        !uuid.test(account) ||
        ids.length !== 4 ||
        ids.some((id) => !uuid.test(id)) ||
        new Set(ids).size !== 4
      )
        throw new Error("Invalid owned observer target");
      downloadStatus.watch(account, ids, playlist);
      target = { account, ids: new Set(ids) };
    },
    arm() {
      if (!target || armed) throw new Error("Invalid owned observer phase");
      armed = true;
    },
    complete: () =>
      counts.acceptedChoices === 4 &&
      counts.failedChoices === 1 &&
      counts.acceptedResets === 4 &&
      counts.chooseRequests === 5 &&
      counts.resetRequests === 4 &&
      commands.size === 8 &&
      workflowConsole.complete() &&
      counts.acceptedReads >= 4,
    async finish(releasePrivate = false) {
      let timer: ReturnType<typeof setTimeout> | undefined;
      try {
        await Promise.race([
          (async () => {
            while (pending.size) await Promise.allSettled([...pending]);
          })(),
          new Promise<void>((resolve) => {
            timer = setTimeout(() => {
              counts.drainTimeout++;
              resolve();
            }, 3000);
          }),
        ]);
      } finally {
        clearTimeout(timer);
      }
      downloadStatus.finalize();
      const safe = workflowConsole.snapshot();
      if (process.env.HITO_ARTIST_IDENTITY_EVIDENCE === "1")
        await writeFile(
          "/evidence/artist-groups.json",
          `${JSON.stringify(safe)}\n`,
          { mode: 0o600 },
        );
      if (releasePrivate) workflowConsole.clear();
    },
  };
}
