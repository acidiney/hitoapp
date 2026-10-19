import type { BrowserContext, Request, Route } from "@playwright/test";
import type { Remember } from "./audiobook-library-client";

export type LibraryNativeWitness = {
  phase: "lost" | "deadline" | "account" | "anchor";
  observed: boolean;
  acceptedStatus: number | null;
  failed: boolean;
};
type Disruption = {
  kind: "lost" | "held";
  href: string;
  observed: boolean;
  release: () => void;
  waiting: Promise<void>;
  request: Request | null;
  witness: LibraryNativeWitness;
};
export async function libraryDelivery(
  context: BrowserContext,
  origin: string,
  remember: Remember,
) {
  let armed = false;
  let sessions = false;
  let closed = false;
  let account = "";
  let planned: Disruption | null = null;
  const owned = new WeakSet<Request>();
  const disruptions: Disruption[] = [];
  const pending = new Set<Promise<void>>();
  const starts = new WeakMap<Request, boolean>();
  const downloads = new WeakSet<Request>();
  const acceptedSessions = new WeakSet<Request>();
  const nativeWitnesses = new Map<string, LibraryNativeWitness>();
  const requestWitnesses = new WeakMap<Request, LibraryNativeWitness>();
  let nativeIntents = 0;
  function nativeIntent(href: string, phase: LibraryNativeWitness["phase"]) {
    const url = new URL(href, origin);
    if (
      ++nativeIntents > 5 ||
      url.origin !== origin ||
      url.pathname !== "/audiobooks" ||
      (url.searchParams.get("cursor")?.length ?? 0) > 4096
    )
      throw new Error("The owned library native diagnostic intent is invalid");
    const witness: LibraryNativeWitness = {
      phase,
      observed: false,
      acceptedStatus: null,
      failed: false,
    };
    nativeWitnesses.set(url.href, witness);
    return witness;
  }
  const counts = {
    requests: 0,
    visits: 0,
    apiBookReads: 0,
    sync: 0,
    audio: 0,
    source: 0,
    downloads: 0,
    downloadStatusReads: 0,
    absentDownloadStatuses: 0,
    foreign: 0,
    unexpectedWrites: 0,
    unexpectedReads: 0,
    failures: 0,
    acceptedSessionResponses: 0,
    sessionCancellations: 0,
    sessionVerificationCancellations: 0,
    ownedFailures: 0,
    preparationFailures: 0,
    preparationCancellations: 0,
    lost: 0,
    held: 0,
    forwarded: 0,
    invalid: 0,
  };
  function track(task: Promise<void>) {
    const run = task
      .catch(() => {
        counts.invalid++;
      })
      .finally(() => pending.delete(run));
    pending.add(run);
    return run;
  }
  context.on("request", (request) => {
    if (nativeWitnesses.get(request.url())?.observed)
      nativeWitnesses.delete(request.url());
    starts.set(request, armed);
    counts.requests++;
  });
  context.on("requestfailed", (request) => {
    const witness = requestWitnesses.get(request);
    if (witness) witness.failed = true;
    const path = new URL(request.url()).pathname;
    const cancelled = /abort|cancel/i.test(request.failure()?.errorText ?? "");
    const verification = request.method() === "GET" && path === "/api/v1/me";
    if (owned.has(request)) counts.ownedFailures++;
    else if (starts.get(request) === false) {
      if (cancelled && (verification || acceptedSessions.has(request)))
        counts.preparationCancellations++;
      else counts.preparationFailures++;
    } else if (sessions && cancelled && acceptedSessions.has(request))
      counts.sessionCancellations++;
    else if (sessions && cancelled && verification)
      counts.sessionVerificationCancellations++;
    else counts.failures++;
  });
  context.on("response", (response) => {
    const request = response.request();
    const witness = requestWitnesses.get(request);
    if (
      witness?.phase === "anchor" &&
      response.status() === 409 &&
      response.headers()["cache-control"] === "private, no-store"
    )
      witness.acceptedStatus = 409;
    const path = new URL(response.url()).pathname;
    const session =
      request.method() === "POST" &&
      ((starts.get(request) === false &&
        path === "/api/v1/setup" &&
        response.status() === 201) ||
        (sessions &&
          ((path === "/api/v1/login" && response.status() === 200) ||
            (path === "/api/v1/logout" && response.status() === 204))));
    if (session) {
      if (response.headers()["cache-control"] !== "private, no-store")
        counts.invalid++;
      acceptedSessions.add(request);
      if (starts.get(request)) counts.acceptedSessionResponses++;
    }
    if (!downloads.has(response.request())) return;
    void track(
      (async () => {
        const body = await response.body();
        if (
          response.status() !== 404 ||
          body.length > 4096 ||
          body.toString() !== JSON.stringify({ error: "Resource not found" }) ||
          response.headers()["cache-control"] !== "private, no-store"
        )
          counts.invalid++;
        else counts.absentDownloadStatuses++;
      })(),
    );
  });
  async function handle(route: Route) {
    const request = route.request();
    const url = new URL(request.url());
    const path = url.pathname;
    const write = !["GET", "HEAD", "OPTIONS"].includes(request.method());
    const foreign = url.origin !== origin;
    const audio =
      request.resourceType() === "media" || /\/audio(?:\/|$)/.test(path);
    const source =
      /\/(?:sources|source-choice|preparations|resolve|successful-playback)(?:\/|$)/.test(
        path,
      );
    const download = /\/(?:download|downloads)(?:\/|$)/.test(path);
    if (foreign) counts.foreign++;
    if (audio) counts.audio++;
    if (source) counts.source++;
    if (armed && path === "/api/v1/sync") counts.sync++;
    if (armed && path === "/api/v1/audiobooks/page") counts.apiBookReads++;
    const status =
      armed &&
      download &&
      !write &&
      request.method() === "GET" &&
      /^\/api\/v1\/recordings\/[a-f0-9-]{36}\/download$/.test(path) &&
      request.headers()["x-hito-request"] === "1" &&
      request.headers()["x-hito-account"] === account;
    if (status) {
      downloads.add(request);
      counts.downloadStatusReads++;
    } else if (download) counts.downloads++;
    const session =
      sessions &&
      ["/api/v1/logout", "/api/v1/login"].includes(path) &&
      request.method() === "POST";
    if (armed && write && !session) counts.unexpectedWrites++;
    const allowedRead =
      path === "/audiobooks" ||
      ["/", "/library", "/api/v1/me", "/api/v1/setup", "/favicon.ico"].includes(
        path,
      ) ||
      path.startsWith("/assets/") ||
      /^\/api\/v1\/recordings\/[a-f0-9-]{36}\/artwork(?:\/availability)?$/.test(
        path,
      ) ||
      status;
    if (armed && !write && !allowedRead) counts.unexpectedReads++;
    if (
      foreign ||
      audio ||
      source ||
      (download && !status) ||
      counts.requests > 250 ||
      (armed && ((write && !session) || (!write && !allowedRead))) ||
      Buffer.byteLength(request.postData() ?? "") > 8192
    ) {
      counts.invalid++;
      await route.abort("blockedbyclient");
      return;
    }
    if (armed && path === "/audiobooks") {
      counts.visits++;
      const witness = nativeWitnesses.get(url.href);
      if (
        witness &&
        !witness.observed &&
        request.method() === "GET" &&
        request.headers()["x-inertia"] === "true" &&
        request.headers()["x-hito-account"] === account
      ) {
        witness.observed = true;
        requestWitnesses.set(request, witness);
      }
      const cursor = url.searchParams.get("cursor");
      if (cursor) {
        if (cursor.length > 4096) counts.invalid++;
        remember(cursor);
      }
    }
    const disruption = planned;
    if (
      !armed ||
      !disruption ||
      disruption.observed ||
      path !== "/audiobooks" ||
      url.pathname + url.search !== disruption.href ||
      request.headers()["x-inertia"] !== "true"
    ) {
      await route.continue();
      return;
    }
    disruption.observed = true;
    disruption.request = request;
    owned.add(request);
    const response = await route.fetch({
      timeout: 10_000,
      maxRedirects: 0,
      maxRetries: 0,
    });
    try {
      counts.forwarded++;
      const body = await response.body();
      if (
        body.length > 524_288 ||
        response.status() !== 200 ||
        response.headers()["cache-control"] !== "private, no-store" ||
        response.headers()["x-inertia"] !== "true"
      )
        throw new Error("The owned Inertia delivery was not accepted");
      const page = JSON.parse(body.toString()) as {
        component?: unknown;
        props?: { user?: { id?: unknown }; books?: unknown[] };
      };
      if (
        page.component !== "AudiobookIndexPage" ||
        page.props?.user?.id !== account ||
        !Array.isArray(page.props.books) ||
        page.props.books.length > 100
      )
        throw new Error(
          "The owned Inertia delivery changed its account or page bound",
        );
      disruption.witness.acceptedStatus = 200;
      if (disruption.kind === "lost") {
        counts.lost++;
        await route.abort("failed");
        return;
      }
      counts.held++;
      await disruption.waiting;
      if (closed) await route.abort("aborted").catch(() => {});
      else {
        try {
          await route.fulfill({ response, body });
        } catch {
          if (!request.failure())
            throw new Error(
              "The owned held delivery failed without cancellation",
            );
        }
      }
    } finally {
      await response.dispose();
    }
  }
  const handler = (route: Route) =>
    track(
      handle(route).catch(async () => {
        counts.invalid++;
        await route.abort("failed").catch(() => {});
      }),
    );
  await context.route("**/*", handler);
  return {
    counts,
    nativeWitness(location: string) {
      return nativeWitnesses.get(location) ?? null;
    },
    expectAnchorConflict(href: string) {
      nativeIntent(href, "anchor");
    },
    arm(id: string) {
      if (armed) throw new Error("The library transport was already armed");
      armed = true;
      account = id;
    },
    sessions(id: string) {
      sessions = true;
      account = id;
    },
    disrupt(kind: "lost" | "held", href: string) {
      if (planned && !planned.observed)
        throw new Error("The previous owned disruption was not exercised");
      let release = () => {};
      const waiting = new Promise<void>((resolve) => {
        release = resolve;
      });
      if (disruptions.length >= 3)
        throw new Error("The library disruption budget was exceeded");
      const disruption: Disruption = {
        kind,
        href,
        observed: false,
        release,
        waiting,
        request: null,
        witness: nativeIntent(
          href,
          kind === "lost" ? "lost" : counts.held === 0 ? "deadline" : "account",
        ),
      };
      planned = disruption;
      disruptions.push(disruption);
      return { observed: () => disruption.observed, release };
    },
    async finish() {
      while (pending.size) await Promise.allSettled([...pending]);
    },
    async close() {
      closed = true;
      for (const disruption of disruptions) disruption.release();
      while (pending.size) await Promise.allSettled([...pending]);
      await context.unroute("**/*", handler).catch(() => {});
    },
  };
}
export type LibraryDelivery = Awaited<ReturnType<typeof libraryDelivery>>;
