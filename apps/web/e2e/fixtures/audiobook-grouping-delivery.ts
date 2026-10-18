import { isDeepStrictEqual } from "node:util";
import type {
  BrowserContext,
  Page,
  Request,
  Response,
  Route,
} from "@playwright/test";
import type { components } from "../../src/generated";

type Schema = components["schemas"];
const uuid = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;

export type FailureDetails = {
  kind: string;
  method: string;
  origin: string;
  phaseStarted: string;
  phaseFailed: string;
  ordinal: number;
  armedAtStart: boolean;
  decision: string;
  resource: string;
  navigation: boolean;
  hasQuery: boolean;
  noBody: boolean;
  requestGuard: boolean;
  inertia: boolean;
  account: string;
  envelopeOrigin: string;
  code: string;
  responseStarted: boolean;
  responseEnded: boolean;
  responseSeen: boolean;
  responseStatus: number;
  responseJson: boolean;
  responsePrivate: boolean;
  responseInspectionFailed: boolean;
};

export function failureScope<T>(
  request: T,
  lostRequest: T | null,
  armedAtStart: boolean | undefined,
): "lost" | "preparation" | "unexpected" {
  if (request === lostRequest)
    return armedAtStart === true ? "lost" : "unexpected";
  return armedAtStart === false ? "preparation" : "unexpected";
}

export async function audiobookGroupingDelivery(
  context: BrowserContext,
  origin: string,
  phase: () => string = () => "unspecified browser phase",
) {
  const pending = new Set<Promise<void>>();
  const statusRequests = new WeakSet<Request>();
  const syncRequests = new WeakSet<Request>();
  const starts = new WeakMap<
    Request,
    { phase: string; armed: boolean; ordinal: number }
  >();
  const decisions = new WeakMap<Request, string>();
  const failures: {
    request: Request;
    phase: string;
    responseSeen: boolean;
    responseStatus: number;
    responseJson: boolean;
    responsePrivate: boolean;
    responseInspectionFailed: boolean;
  }[] = [];
  let target: {
    account: string;
    detail: Schema["AudiobookDetail"];
    downloads: Set<string>;
    recordings: string[];
    libraries: string[];
  } | null = null;
  let lost: Request | null = null;
  let replayAccepted = false;
  let detailAccepted = false;
  const counts = {
    requests: 0,
    foreign: 0,
    audio: 0,
    download: 0,
    catalog: 0,
    sourceWrites: 0,
    unexpectedWrites: 0,
    lostFailures: 0,
    rawApiFailures: 0,
    preparationApiFailures: 0,
    unexpectedFailures: 0,
    unexpectedOtherFailures: 0,
    syncPosts: 0,
    forwarded: 0,
    acceptedOriginal: 0,
    acceptedReplay: 0,
    detailReads: 0,
    bookVisits: 0,
    downloadStatusReads: 0,
    absentStatuses: 0,
  };
  const state = {
    failed: false,
    bodies: [] as string[],
    batch: null as Schema["SyncBatch"] | null,
    receipt: null as Schema["SyncReceipt"] | null,
    order: [] as string[],
  };
  function track(task: Promise<void>) {
    const owned = task
      .catch(() => {
        state.failed = true;
      })
      .finally(() => pending.delete(owned));
    pending.add(owned);
    return owned;
  }
  context.on("request", (request) => {
    counts.requests++;
    starts.set(request, {
      phase: phase().slice(0, 120),
      armed: target !== null,
      ordinal: counts.requests,
    });
  });
  context.on("requestfailed", (request) => {
    const scope = failureScope(request, lost, starts.get(request)?.armed);
    if (scope === "lost") {
      counts.lostFailures++;
      return;
    }
    const api = new URL(request.url()).pathname.startsWith("/api/v1/");
    if (api) {
      counts.rawApiFailures++;
      if (scope === "preparation") counts.preparationApiFailures++;
      else counts.unexpectedFailures++;
    } else if (scope === "unexpected") counts.unexpectedOtherFailures++;
    if (scope === "unexpected") state.failed = true;
    if (api || scope === "unexpected") {
      if (failures.length >= 8) return;
      const observation = {
        request,
        phase: phase().slice(0, 120),
        responseSeen: false,
        responseStatus: 0,
        responseJson: false,
        responsePrivate: false,
        responseInspectionFailed: false,
      };
      failures.push(observation);
      void track(
        (async () => {
          try {
            const response = await request.response();
            observation.responseSeen = response !== null;
            if (response) {
              observation.responseStatus = response.status();
              observation.responseJson =
                response
                  .headers()
                  ["content-type"]?.startsWith("application/json") ?? false;
              observation.responsePrivate =
                response.headers()["cache-control"] === "private, no-store";
            }
          } catch {
            observation.responseInspectionFailed = true;
          }
        })(),
      );
    }
  });
  context.on("response", (response) => {
    const request = response.request();
    const path = new URL(response.url()).pathname;
    if (syncRequests.has(request) && request !== lost) {
      if (response.status() !== 200) state.failed = true;
      replayAccepted = response.status() === 200;
      state.order.push("retry_response");
      void track(
        (async () => {
          const body = await boundedBody(response, 8192);
          if (
            !state.receipt ||
            !isDeepStrictEqual(JSON.parse(body.toString()), state.receipt) ||
            response.headers()["cache-control"] !== "private, no-store"
          )
            state.failed = true;
          else counts.acceptedReplay++;
        })(),
      );
    }
    if (target && path === `/api/v1/audiobooks/${target.detail.playlist.id}`) {
      detailAccepted = replayAccepted && response.status() === 200;
      state.order.push("detail_response");
      const expected = target.detail;
      void track(
        (async () => {
          const body = await boundedBody(response, 524_288);
          if (
            !detailAccepted ||
            !isDeepStrictEqual(JSON.parse(body.toString()), expected) ||
            response.headers()["cache-control"] !== "private, no-store"
          )
            state.failed = true;
        })(),
      );
    }
    if (statusRequests.has(request)) {
      void track(
        (async () => {
          const body = await boundedBody(response, 4096);
          if (
            response.status() !== 404 ||
            body.toString() !==
              JSON.stringify({ error: "Resource not found" }) ||
            !response.headers()["content-type"]?.startsWith("application/json")
          )
            state.failed = true;
          else counts.absentStatuses++;
        })(),
      );
    }
  });

  async function handle(route: Route) {
    const request = route.request();
    const url = new URL(request.url());
    const path = url.pathname;
    const writing = !["GET", "HEAD", "OPTIONS"].includes(request.method());
    decisions.set(request, "observed");
    const foreign = url.origin !== origin;
    const audio =
      request.resourceType() === "media" || /\/audio(?:\/|$)/.test(path);
    const catalog =
      /\/artist-review\/|\/artist-associations\/|\/plugins\/[^/]+\/(?:catalog|search|artists|recordings|playlists)(?:\/|$)/.test(
        path,
      );
    const source =
      writing && /\/(?:sources|source-choice|preparations)(?:\/|$)/.test(path);
    const download = /\/(?:download|downloads)(?:\/|$)/.test(path);
    const status =
      download &&
      target &&
      !foreign &&
      !url.search &&
      request.method() === "GET" &&
      request.postData() === null &&
      target.downloads.has(path) &&
      request.headers()["x-hito-request"] === "1" &&
      request.headers()["x-hito-account"] === target.account;
    if (foreign) counts.foreign++;
    if (audio) counts.audio++;
    if (catalog) counts.catalog++;
    if (source) counts.sourceWrites++;
    if (download && !status) counts.download++;
    if (status) {
      counts.downloadStatusReads++;
      statusRequests.add(request);
    }
    const unexpectedWrite =
      target !== null && writing && path !== "/api/v1/sync";
    if (unexpectedWrite) counts.unexpectedWrites++;
    const body = request.postData();
    if (
      foreign ||
      audio ||
      catalog ||
      source ||
      (download && !status) ||
      unexpectedWrite ||
      counts.requests > 250 ||
      (body !== null && Buffer.byteLength(body) > 262_144)
    ) {
      state.failed = true;
      decisions.set(request, "policy_abort");
      await route.abort();
      return;
    }
    if (target && path === `/api/v1/audiobooks/${target.detail.playlist.id}`) {
      counts.detailReads++;
      if (
        !replayAccepted ||
        writing ||
        request.headers()["x-hito-account"] !== target.account
      )
        state.failed = true;
      state.order.push("detail_request");
    }
    if (
      target &&
      path === `/audiobooks/${target.detail.playlist.id}` &&
      (request.isNavigationRequest() ||
        request.headers()["x-inertia"] === "true")
    ) {
      counts.bookVisits++;
      if (!detailAccepted) state.failed = true;
      state.order.push("book_visit");
    }
    if (!target || path !== "/api/v1/sync" || request.method() !== "POST") {
      decisions.set(request, "continued");
      await route.continue();
      return;
    }
    counts.syncPosts++;
    if (
      !body ||
      Buffer.byteLength(body) > 4096 ||
      state.bodies.length >= 2 ||
      request.headers()["origin"] !== origin ||
      request.headers()["x-hito-request"] !== "1" ||
      request.headers()["x-hito-account"] !== target.account
    ) {
      state.failed = true;
      decisions.set(request, "sync_envelope_abort");
      await route.abort();
      return;
    }
    const batch = JSON.parse(body) as Schema["SyncBatch"];
    if (
      !uuid.test(batch.device_id) ||
      batch.operations?.length !== 1 ||
      !uuid.test(batch.operations[0].id) ||
      !isDeepStrictEqual(batch.operations[0].action, {
        kind: "audiobook_create",
        playlist_id: target.detail.playlist.id,
      })
    ) {
      state.failed = true;
      decisions.set(request, "sync_identity_abort");
      await route.abort();
      return;
    }
    state.bodies.push(body);
    syncRequests.add(request);
    if (lost) {
      if (body !== state.bodies[0] || counts.acceptedOriginal !== 1)
        state.failed = true;
      state.order.push("retry_request");
      decisions.set(request, "continued_retry");
      await route.continue();
      return;
    }
    lost = request;
    state.batch = batch;
    let response: Awaited<ReturnType<typeof route.fetch>> | null = null;
    try {
      counts.forwarded++;
      response = await route.fetch({
        timeout: 10_000,
        maxRetries: 0,
        maxRedirects: 0,
      });
      const bytes = await response.body();
      if (
        bytes.length > 8192 ||
        response.status() !== 200 ||
        response.headers()["cache-control"] !== "private, no-store"
      )
        throw new Error("The owned original sync response was not accepted");
      const receipt = JSON.parse(bytes.toString()) as Schema["SyncReceipt"];
      if (
        !isDeepStrictEqual(receipt, { acknowledged: [batch.operations[0].id] })
      )
        throw new Error(
          "The owned sync acknowledgement did not match its operation",
        );
      state.receipt = receipt;
      counts.acceptedOriginal++;
      state.order.push("original_accepted");
    } finally {
      if (response) await response.dispose();
      decisions.set(request, "owned_lost_delivery");
      await route.abort("failed");
    }
  }
  await context.route("**/*", (route) =>
    track(
      handle(route).catch(async () => {
        state.failed = true;
        decisions.set(route.request(), "handler_failure_abort");
        try {
          await route.abort("failed");
        } catch {}
      }),
    ),
  );
  function failureDetails(): FailureDetails[] {
    const staticPaths = new Map([
      ["/api/v1/me", "account_self"],
      ["/api/v1/setup", "setup"],
      ["/api/v1/plugins", "plugin_settings"],
      ["/api/v1/metadata/settings", "metadata_settings"],
      ["/api/v1/sync", "sync"],
      ["/api/v1/recordings", "recording_list"],
      ["/api/v1/recordings/progress", "recording_progress"],
      ["/api/v1/artists", "artist_list"],
      ["/api/v1/releases", "release_list"],
      ["/api/v1/libraries", "library_list"],
      ["/api/v1/activity", "activity"],
      ["/api/v1/player", "player_status"],
    ]);
    if (target) {
      for (const library of target.libraries)
        staticPaths.set(
          `/api/v1/libraries/${library}/folders`,
          "owned_folder_list",
        );
      staticPaths.set(
        `/api/v1/audiobooks/${target.detail.playlist.id}`,
        "owned_book_detail",
      );
      staticPaths.set(
        `/api/v1/playlists/${target.detail.playlist.id}/download`,
        "owned_playlist_download",
      );
      for (const [index, recording] of target.recordings.entries()) {
        const label =
          index === 0
            ? "first_recording"
            : index === 1
              ? "second_recording"
              : "owned_recording";
        for (const [suffix, kind] of [
          ["artwork", "artwork"],
          ["artwork/availability", "artwork_availability"],
          ["sources", "sources"],
          ["source-choice", "source_choice"],
          ["download", "download"],
          ["metadata", "metadata"],
        ])
          staticPaths.set(
            `/api/v1/recordings/${recording}/${suffix}`,
            `${label}_${kind}`,
          );
      }
    }
    return failures.map((failure) => {
      const request = failure.request;
      const url = new URL(request.url());
      const headers = request.headers();
      const error = request.failure()?.errorText ?? "";
      const timing = request.timing();
      const start = starts.get(request);
      return {
        kind: staticPaths.get(url.pathname) ?? "other",
        method: [
          "GET",
          "POST",
          "PUT",
          "DELETE",
          "PATCH",
          "HEAD",
          "OPTIONS",
        ].includes(request.method())
          ? request.method()
          : "other",
        origin: url.origin === origin ? "owned" : "other",
        phaseStarted: start?.phase ?? "unobserved",
        phaseFailed: failure.phase,
        ordinal: start?.ordinal ?? 0,
        armedAtStart: start?.armed ?? false,
        decision: decisions.get(request) ?? "unobserved",
        resource: [
          "fetch",
          "xhr",
          "image",
          "document",
          "script",
          "stylesheet",
          "media",
          "other",
        ].includes(request.resourceType())
          ? request.resourceType()
          : "other",
        navigation: request.isNavigationRequest(),
        hasQuery: Boolean(url.search),
        noBody: request.postData() === null,
        requestGuard: headers["x-hito-request"] === "1",
        inertia: headers["x-inertia"] === "true",
        account: !headers["x-hito-account"]
          ? "absent"
          : target && headers["x-hito-account"] === target.account
            ? "owned"
            : "other",
        envelopeOrigin: !headers.origin
          ? "absent"
          : headers.origin === origin
            ? "owned"
            : "other",
        code:
          error === "net::ERR_ABORTED"
            ? "chromium_aborted"
            : error === "NS_BINDING_ABORTED"
              ? "binding_aborted"
              : ["cancelled", "canceled", "Load request cancelled"].includes(
                    error,
                  )
                ? "cancelled"
                : error === "net::ERR_FAILED"
                  ? "chromium_failed"
                  : "other",
        responseStarted: timing.responseStart >= 0,
        responseEnded: timing.responseEnd >= 0,
        responseSeen: failure.responseSeen,
        responseStatus: failure.responseStatus,
        responseJson: failure.responseJson,
        responsePrivate: failure.responsePrivate,
        responseInspectionFailed: failure.responseInspectionFailed,
      };
    });
  }
  return {
    state,
    counts,
    arm(
      account: string,
      detail: Schema["AudiobookDetail"],
      recordings: string[],
      libraries: string[],
    ) {
      if (
        target ||
        !uuid.test(account) ||
        !uuid.test(detail.playlist.id) ||
        libraries.some((library) => !uuid.test(library))
      )
        throw new Error("The owned audiobook delivery target is invalid");
      target = {
        account,
        detail,
        recordings: [...recordings],
        libraries: [...libraries],
        downloads: new Set([
          `/api/v1/playlists/${detail.playlist.id}/download`,
          ...recordings.map((id) => `/api/v1/recordings/${id}/download`),
        ]),
      };
    },
    failureDetails,
    async finish() {
      while (pending.size) await Promise.allSettled([...pending]);
    },
  };
}

async function boundedBody(response: Response, limit: number) {
  const body = await response.body();
  if (body.length > limit)
    throw new Error("The owned audiobook response exceeded its bound");
  return body;
}

export async function audiobookDownloadAbsence(page: Page, playlist: string) {
  const response = await page.request.get(
    `/api/v1/playlists/${playlist}/download`,
    { timeout: 10_000 },
  );
  try {
    const body = await response.body();
    if (
      response.status() !== 404 ||
      body.length > 4096 ||
      body.toString() !==
        JSON.stringify({
          error: "Resource not found",
        } satisfies Schema["ErrorResponse"])
    )
      throw new Error("The owned audiobook must have no implicit download");
    return { status: 404, absent: true };
  } finally {
    await response.dispose();
  }
}
