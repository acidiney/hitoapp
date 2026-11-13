import { randomUUID } from "node:crypto";
import type { IncomingMessage, ServerResponse } from "node:http";
import type { APIRequestContext } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import { listenOwned } from "./owned-http-service";
import {
  lyricsProvider,
  plainCandidate,
  type LyricsCandidate,
} from "./lyrics-provider";
import { providerFixture } from "./provider";

export type RouteOutcome = { route: string; outcome: "accepted" | "rejected" };
export type HealthMode = "protocol" | "absent";

type Inner = { address: string; token: string; close(): Promise<void> };

const passedHeaders = ["content-type", "content-range", "accept-ranges"];

function newToken(): string {
  return `credential-e2e-${randomUUID()}`;
}

async function readText(request: IncomingMessage): Promise<string> {
  let text = "";
  for await (const chunk of request) text += chunk;
  return text;
}

async function serveCredentialPlugin(
  label: string,
  inner: Inner,
  health: HealthMode,
) {
  const manifestId = `org.hito.credential-e2e-${randomUUID().slice(0, 8)}`;
  const state = {
    expected: newToken(),
    manifestOpen: false,
    health,
    openRoutes: new Set<string>(),
  };
  const initial = state.expected;
  const log: RouteOutcome[] = [];
  const abandoned: string[] = [];
  const holds = new Map<string, Promise<void>>();
  const releases = new Map<string, () => void>();
  const arrivals = new Map<string, Promise<void>>();
  const arrived = new Map<string, () => void>();

  async function forward(
    request: IncomingMessage,
    response: ServerResponse,
    route: string,
    search: string,
    text: string,
  ) {
    const headers: Record<string, string> = {
      authorization: `Bearer ${inner.token}`,
    };
    for (const name of ["content-type", "range"]) {
      const value = request.headers[name];
      if (typeof value === "string") headers[name] = value;
    }
    const upstream = await fetch(new URL(route + search, inner.address), {
      method: request.method,
      headers,
      body: text.length > 0 ? text : undefined,
    });
    if (route === "v1/manifest") {
      const manifest = (await upstream.json()) as Schema<"Manifest">;
      response
        .writeHead(upstream.status, { "content-type": "application/json" })
        .end(JSON.stringify({ ...manifest, id: manifestId, name: label }));
      return;
    }
    const outgoing: Record<string, string> = {};
    for (const name of passedHeaders) {
      const value = upstream.headers.get(name);
      if (value) outgoing[name] = value;
    }
    const length = upstream.headers.get("content-length");
    if (request.method === "HEAD" && length)
      outgoing["content-length"] = length;
    response
      .writeHead(upstream.status, outgoing)
      .end(Buffer.from(await upstream.arrayBuffer()));
  }

  async function handle(request: IncomingMessage, response: ServerResponse) {
    const url = new URL(request.url!, "http://fixture.invalid");
    const route = url.pathname.replace(/^\/plugin\//, "");
    const text = await readText(request);
    response.on("close", () => {
      if (!response.writableFinished) abandoned.push(route);
    });
    const pending = holds.get(route);
    if (pending) {
      arrived.get(route)?.();
      await pending;
    }
    if (route === "v1/health" && state.health === "absent") {
      response.writeHead(404).end();
      return;
    }
    const open =
      (route === "v1/manifest" && state.manifestOpen) ||
      state.openRoutes.has(route);
    if (!open && request.headers.authorization !== `Bearer ${state.expected}`) {
      log.push({ route, outcome: "rejected" });
      response.writeHead(401).end();
      return;
    }
    log.push({ route, outcome: "accepted" });
    if (route === "v1/health") {
      response
        .writeHead(200, { "content-type": "application/json" })
        .end(JSON.stringify({ state: "ok", reason: null, since: null }));
      return;
    }
    await forward(request, response, route, url.search, text);
  }

  const server = await listenOwned("credential-plugin", (request, response) => {
    void handle(request, response).catch(() => {
      if (!response.headersSent) response.writeHead(502);
      response.end();
    });
  });

  const release = (route: string) => {
    holds.delete(route);
    releases.get(route)?.();
  };
  const entries = (route: string, outcome: RouteOutcome["outcome"]) =>
    log.filter((item) => item.route === route && item.outcome === outcome);

  return {
    label,
    address: `http://127.0.0.1:${server.port}/plugin/`,
    initialToken: initial,
    state,
    log,
    abandoned,
    currentToken: () => state.expected,
    rotate: () => {
      state.expected = newToken();
      return state.expected;
    },
    openRoute: (route: string) => {
      state.openRoutes.add(route);
    },
    restore: () => {
      state.expected = initial;
    },
    hold: (route: string) => {
      holds.set(
        route,
        new Promise<void>((resolve) => releases.set(route, resolve)),
      );
      arrivals.set(
        route,
        new Promise<void>((resolve) => arrived.set(route, resolve)),
      );
    },
    held: (route: string) => arrivals.get(route)!,
    release,
    accepted: (route: string) => entries(route, "accepted").length,
    rejected: (route: string) => entries(route, "rejected").length,
    close: async () => {
      for (const route of [...holds.keys()]) release(route);
      await server.close(10_000);
      await inner.close();
    },
  };
}

export async function credentialLyricsPlugin(
  label: string,
  health: HealthMode = "protocol",
) {
  const inner = await lyricsProvider();
  const plugin = await serveCredentialPlugin(
    label,
    {
      address: inner.address,
      token: inner.token,
      close: inner.close,
    },
    health,
  );
  return { ...plugin, inner };
}

export async function credentialSourcesPlugin(
  label: string,
  health: HealthMode = "protocol",
) {
  const inner = await providerFixture(label);
  const plugin = await serveCredentialPlugin(
    label,
    {
      address: inner.address,
      token: inner.token,
      close: inner.close,
    },
    health,
  );
  return { ...plugin, inner };
}

export type CredentialPlugin = Awaited<
  ReturnType<typeof credentialLyricsPlugin>
>;

const mutation = (origin: string) => ({ "x-hito-request": "1", origin });

export async function offerService(
  administrator: APIRequestContext,
  origin: string,
  name: string,
  address: string,
): Promise<Schema<"PluginService">> {
  const offered = await administrator.post("/api/v1/plugins/services", {
    headers: mutation(origin),
    data: { name, base_url: address } satisfies Schema<"OfferPluginService">,
  });
  if (offered.status() !== 201) throw new Error("The service was refused");
  return (await offered.json()) as Schema<"PluginService">;
}

export async function installConnection(
  request: APIRequestContext,
  origin: string,
  service: Schema<"PluginService">,
  token: string,
): Promise<Schema<"PluginInstallation">> {
  const installed = await request.post("/api/v1/plugins", {
    headers: mutation(origin),
    data: {
      service_id: service.id,
      service_revision: service.revision,
      bearer_token: token,
    } satisfies Schema<"InstallPlugin">,
  });
  if (installed.status() !== 201) throw new Error("The connection was refused");
  return (await installed.json()) as Schema<"PluginInstallation">;
}

export async function pluginSettings(request: APIRequestContext) {
  const read = await request.get("/api/v1/plugins");
  if (read.status() !== 200) throw new Error("Plugin settings were refused");
  return (await read.json()) as Schema<"PluginSettings">;
}

export async function bumpRevision(
  request: APIRequestContext,
  origin: string,
  installation: Schema<"PluginInstallation">,
): Promise<Schema<"PluginInstallation">> {
  const current = (await pluginSettings(request)).installations.find(
    (item) => item.id === installation.id,
  );
  const refreshed = await request.post(
    `/api/v1/plugins/${installation.id}/refresh`,
    {
      headers: mutation(origin),
      data: {
        revision: current!.revision,
        credential: { action: "keep" },
      } satisfies Schema<"RefreshPlugin">,
    },
  );
  const result = (await refreshed.json()) as Schema<"PluginRefresh">;
  if (refreshed.status() !== 200 || result.kind !== "updated")
    throw new Error("The connection did not refresh");
  return result.installation;
}

export async function removeConnection(
  request: APIRequestContext,
  origin: string,
  installation: Schema<"PluginInstallation">,
) {
  const current = (await pluginSettings(request)).installations.find(
    (item) => item.id === installation.id,
  );
  if (!current) return;
  await request.delete(`/api/v1/plugins/${installation.id}`, {
    headers: mutation(origin),
    data: { revision: current.revision },
  });
}

export async function withdrawService(
  administrator: APIRequestContext,
  origin: string,
  service: Schema<"PluginService">,
) {
  const current = (await pluginSettings(administrator)).services.find(
    (item) => item.id === service.id,
  );
  if (!current) return;
  await administrator.delete(`/api/v1/plugins/services/${service.id}`, {
    headers: mutation(origin),
    data: { revision: current.revision },
  });
}

export async function saveLyricIdentity(
  request: APIRequestContext,
  origin: string,
  recording: string,
  installation: Schema<"PluginInstallation">,
  providerRecordingId: string,
): Promise<Schema<"RecordingLyricIdentity">> {
  const candidate = {
    installation_id: installation.id,
    installation_revision: installation.revision,
    provider_recording_id: providerRecordingId,
  } satisfies Schema<"ProviderCandidate">;
  const base = `/api/v1/recordings/${recording}/lyrics/identities`;
  const reviewed = await request.post(`${base}/review`, {
    headers: mutation(origin),
    data: candidate,
  });
  if (reviewed.status() !== 200) throw new Error("The review was refused");
  const review = (await reviewed.json()) as Schema<"RecordingLyricReview">;
  const retained = await request.post(base, {
    headers: mutation(origin),
    data: {
      candidate,
      review_stamp: review.review_stamp,
    } satisfies Schema<"RetainRecordingLyricIdentity">,
  });
  if (!retained.ok()) throw new Error("The lyric choice was refused");
  return (await retained.json()) as Schema<"RecordingLyricIdentity">;
}

export async function listLyricIdentities(
  request: APIRequestContext,
  recording: string,
) {
  const listed = await request.get(
    `/api/v1/recordings/${recording}/lyrics/identities`,
  );
  return ((await listed.json()) as Schema<"RecordingLyricIdentities">).items;
}

export async function removeLyricIdentities(
  request: APIRequestContext,
  origin: string,
  recording: string,
) {
  for (const identity of await listLyricIdentities(request, recording)) {
    if (identity.selector.kind !== "reviewed") continue;
    await request.delete(
      `/api/v1/recordings/${recording}/lyrics/identities/${identity.selector.identity_id}?revision=${identity.selector.identity_revision}`,
      { headers: mutation(origin) },
    );
  }
}

export async function connectCredentialPlugin(
  administrator: APIRequestContext,
  request: APIRequestContext,
  origin: string,
  plugin: {
    label: string;
    address: string;
    initialToken: string;
    close(): Promise<void>;
  },
) {
  let service: Awaited<ReturnType<typeof offerService>> | undefined;
  try {
    const offered = await offerService(
      administrator,
      origin,
      plugin.label,
      plugin.address,
    );
    service = offered;
    const installation = await installConnection(
      request,
      origin,
      offered,
      plugin.initialToken,
    );
    return {
      service: offered,
      installation,
      dispose: async () => {
        try {
          await removeConnection(request, origin, installation);
          await withdrawService(administrator, origin, offered);
        } finally {
          await plugin.close();
        }
      },
    };
  } catch (error) {
    const failures: unknown[] = [];
    if (service)
      await withdrawService(administrator, origin, service).catch((failure) =>
        failures.push(failure),
      );
    await plugin.close().catch((failure) => failures.push(failure));
    for (const failure of failures)
      console.warn("Credential plugin cleanup failed", failure);
    throw error;
  }
}

export const savedLyricId = "credential-saved";

export async function connectSavedLyrics(
  administrator: APIRequestContext,
  request: APIRequestContext,
  origin: string,
  track: {
    id: string;
    facts: Pick<LyricsCandidate, "title" | "artist" | "duration_ms">;
  },
  label: string,
  health: HealthMode = "protocol",
) {
  const plugin = await credentialLyricsPlugin(label, health);
  plugin.inner.state.candidates = [plainCandidate(savedLyricId, track.facts)];
  try {
    const connection = await connectCredentialPlugin(
      administrator,
      request,
      origin,
      plugin,
    );
    try {
      const identity = await saveLyricIdentity(
        request,
        origin,
        track.id,
        connection.installation,
        savedLyricId,
      );
      return {
        plugin,
        service: connection.service,
        installation: connection.installation,
        identity,
        dispose: async () => {
          try {
            await removeLyricIdentities(request, origin, track.id);
          } finally {
            await connection.dispose();
          }
        },
      };
    } catch (error) {
      await connection.dispose();
      throw error;
    }
  } catch (error) {
    await plugin.close().catch(() => undefined);
    throw error;
  }
}
