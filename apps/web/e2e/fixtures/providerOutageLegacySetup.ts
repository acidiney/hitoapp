import { randomUUID } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { expect, type Page } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import { seedPreferencesChanges } from "./metadata-preferences-state";
import type { OutageReader, OutageSetupPhase } from "./providerOutageLibrary";
import type { OutageProvider } from "./providerOutageProvider";

const importPhases = {
  retry: "import_retry",
  queue: "import_queue",
  preserved: "import_preserved",
} as const satisfies Record<string, OutageSetupPhase>;

async function legacyWrite<T>(
  page: Page,
  account: string | null,
  method: "post" | "put",
  path: string,
  data: unknown,
  status: number,
): Promise<T> {
  const response = await page.request[method](path, {
    headers: {
      "x-hito-request": "1",
      ...(account ? { "x-hito-account": account } : {}),
      Origin: new URL(page.url()).origin,
    },
    data,
    timeout: 10_000,
  });
  try {
    const bytes = await response.body();
    if (response.status() !== status || bytes.length > 524_288)
      throw new Error("Owned legacy setup write rejected");
    return JSON.parse(bytes.toString()) as T;
  } finally {
    await response.dispose();
  }
}

async function connectLegacyFolders(
  page: Page,
  read: OutageReader,
  user: Schema<"User">,
) {
  const libraries = await read<Schema<"Library">[]>("/api/v1/libraries");
  const shared = libraries.filter((library) => !library.personal);
  if (shared.length !== 1) throw new Error("Owned shared library missing");
  const library = shared[0];
  const roots = await read<{ id: string; name: string; available: boolean }[]>(
    "/api/v1/storage/directories",
  );
  for (const name of ["Music", "Archive"]) {
    const matches = roots.filter(
      (root) => root.available && root.name === name,
    );
    if (matches.length !== 1) throw new Error("Owned fixture folder missing");
    const directory = { directory_id: matches[0].id };
    const listing = await legacyWrite<{ directory: Schema<"DirectoryRef"> }>(
      page,
      user.id,
      "post",
      "/api/v1/storage/browse",
      { directory, offset: null },
      200,
    );
    if (listing.directory.directory_id !== directory.directory_id)
      throw new Error("Owned fixture folder reference changed");
    const folder = await legacyWrite<Schema<"MusicFolder">>(
      page,
      user.id,
      "post",
      `/api/v1/libraries/${library.id}/folders`,
      { directory },
      201,
    );
    if (folder.library_id !== library.id || !folder.latest_job)
      throw new Error("Owned fixture folder scan not queued");
  }
  await expect
    .poll(
      async () =>
        (
          await read<Schema<"MusicFolder">[]>(
            `/api/v1/libraries/${library.id}/folders`,
          )
        ).map((folder) => [
          folder.name,
          folder.latest_job?.state,
          folder.latest_job?.progress,
        ]),
      { timeout: 20_000 },
    )
    .toEqual(
      expect.arrayContaining([
        ["Music", "complete", 1],
        ["Archive", "complete", 1],
      ]),
    );
}

export async function prepareLegacyOutageSetup(
  page: Page,
  read: OutageReader,
  provider: OutageProvider,
  password: string,
  checkpoint: (phase: OutageSetupPhase) => void,
) {
  checkpoint("administrator");
  await page.goto("/");
  await legacyWrite<{ user: Schema<"User"> }>(
    page,
    null,
    "post",
    "/api/v1/setup",
    { username: "admin", password },
    201,
  );
  const user = await read<Schema<"User">>("/api/v1/me");
  checkpoint("preferences");
  const settings = await read<Schema<"MetadataSettings">>(
    "/api/v1/metadata/settings",
  );
  const disabled = await legacyWrite<Schema<"MetadataSettings">>(
    page,
    user.id,
    "put",
    "/api/v1/metadata/settings",
    { ...settings, enabled: false },
    200,
  );
  const expected = {
    ...settings,
    enabled: false,
    revision: settings.revision + 1,
  };
  if (
    !isDeepStrictEqual(disabled, expected) ||
    !isDeepStrictEqual(
      await read<Schema<"MetadataSettings">>("/api/v1/metadata/settings"),
      expected,
    )
  )
    throw new Error("Owned metadata settings mismatch");
  checkpoint("local_library");
  await connectLegacyFolders(page, read, user);
  const local = await seedPreferencesChanges(page, read, user);
  checkpoint("plugin_offer");
  const offered = await legacyWrite<Schema<"PluginService">>(
    page,
    user.id,
    "post",
    "/api/v1/plugins/services",
    { base_url: provider.address },
    201,
  );
  if (
    offered.name !== provider.manifest.name ||
    offered.base_url !== provider.address
  )
    throw new Error("Owned provider declaration mismatch");
  checkpoint("plugin_review");
  const review = {
    service_id: offered.id,
    service_revision: offered.revision,
    bearer_token: provider.token,
  };
  const preview = await legacyWrite<Schema<"PluginInstallationPreview">>(
    page,
    user.id,
    "post",
    "/api/v1/plugins/preview",
    review,
    200,
  );
  if (
    !preview.digest ||
    !isDeepStrictEqual(preview.manifest, provider.manifest)
  )
    throw new Error("Owned provider preview mismatch");
  checkpoint("plugin_connect");
  const connection = await legacyWrite<Schema<"PluginInstallation">>(
    page,
    user.id,
    "post",
    "/api/v1/plugins",
    { ...review, accept_manifest_digest: preview.digest },
    201,
  );
  if (
    !connection.credential_set ||
    connection.service_id !== offered.id ||
    !isDeepStrictEqual(connection.manifest, provider.manifest)
  )
    throw new Error("Owned provider approval mismatch");
  const copies: Record<string, Schema<"PlaylistDetail">> = {};
  for (const key of ["retry", "queue", "preserved"] as const) {
    checkpoint(importPhases[key]);
    const sent = {
      request_id: randomUUID(),
      installation_revision: connection.revision,
      playlist_reference: key,
    } satisfies Schema<"ImportProviderPlaylist">;
    const imported = await legacyWrite<Schema<"PlaylistImportReceipt">>(
      page,
      user.id,
      "post",
      `/api/v1/plugins/${connection.id}/playlists/import`,
      sent,
      200,
    );
    expect(imported.request_id === sent.request_id).toBe(true);
    copies[key] = await read<Schema<"PlaylistDetail">>(
      `/api/v1/playlists/${imported.playlist_id}`,
    );
    expect(copies[key].entries.every((entry) => !entry.available)).toBe(true);
  }
  return { user, local, connection, copies };
}
