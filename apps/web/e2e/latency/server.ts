import type { APIRequestContext } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import type { Provider } from "./fake-provider.mjs";

export const credentials = {
  username: "bench-admin",
  password: "latency benchmark password",
};

export function writeHeaders(origin: string) {
  return { "x-hito-request": "1", origin };
}

export async function prepareLibrary(
  request: APIRequestContext,
  origin: string,
) {
  const headers = writeHeaders(origin);
  const created = await request.post("/api/v1/setup", {
    headers,
    data: credentials,
  });
  if (created.status() !== 201)
    throw new Error(`Setup failed with ${created.status()}`);
  const libraries: Schema<"Library">[] = await (
    await request.get("/api/v1/libraries")
  ).json();
  const library = libraries[0];
  const connected = await request.post(
    `/api/v1/libraries/${library.id}/folders`,
    {
      headers,
      data: {
        directory: { directory_id: "bench-music", components: [] },
        name: "Music",
      } satisfies Schema<"ConnectFolder">,
    },
  );
  if (connected.status() !== 201)
    throw new Error(`Folder connection failed with ${connected.status()}`);
  const started = performance.now();
  for (;;) {
    const folders: Schema<"MusicFolder">[] = await (
      await request.get(`/api/v1/libraries/${library.id}/folders`)
    ).json();
    const state = folders[0]?.latest_job?.state;
    if (state === "complete") break;
    if (state === "failed") throw new Error("The library scan failed");
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }
  return {
    library: library.id,
    scanSeconds: (performance.now() - started) / 1000,
  };
}

export async function recordingTotal(request: APIRequestContext) {
  const page = await (await request.get("/api/v1/recordings?limit=1")).json();
  return page as Record<string, unknown>;
}

export async function connectProvider(
  request: APIRequestContext,
  origin: string,
  provider: Provider,
) {
  const headers = writeHeaders(origin);
  const offered = await request.post("/api/v1/plugins/services", {
    headers,
    data: {
      name: "Bench provider",
      base_url: provider.address,
    } satisfies Schema<"OfferPluginService">,
  });
  if (offered.status() !== 201)
    throw new Error(`Service offer failed with ${offered.status()}`);
  const service: Schema<"PluginService"> = await offered.json();
  const installed = await request.post("/api/v1/plugins", {
    headers,
    data: {
      service_id: service.id,
      service_revision: service.revision,
      bearer_token: provider.token,
    } satisfies Schema<"InstallPlugin">,
  });
  if (installed.status() !== 201)
    throw new Error(`Plugin installation failed with ${installed.status()}`);
  const connection: Schema<"PluginInstallation"> = await installed.json();
  return async (providerRecordingId: string) => {
    const adopted = await request.post(
      `/api/v1/plugins/${connection.id}/recordings`,
      {
        headers,
        data: {
          installation_revision: connection.revision,
          provider_recording_id: providerRecordingId,
        } satisfies Schema<"AdoptProviderRecording">,
      },
    );
    if (adopted.status() !== 200)
      throw new Error(`Adoption failed with ${adopted.status()}`);
    return (await adopted.json()) as Schema<"Recording">;
  };
}
