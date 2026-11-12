import { randomUUID } from "node:crypto";
import { openPlaylistImport } from "./playlist-dialogs";
import { isDeepStrictEqual } from "node:util";
import { expect, type Page } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import { artistReviewReader } from "./artist-review-state";
import {
  addMetadataProvider,
  openPreferences,
  removeMetadataProvider,
  preferencesAction,
  preferencesLibraryState,
  savePreferences,
  seedPreferencesLibrary,
} from "./metadata-preferences-state";
import { reviewPluginInstallation } from "./plugin-installation-review";
import type { artistIdentityGroupsProvider } from "./artist-identity-groups-provider";
import { openLibrarySection } from "./library-sections";

export type GroupReader = ReturnType<typeof artistReviewReader>;
type Provider = Awaited<ReturnType<typeof artistIdentityGroupsProvider>>;

export async function prepareArtistGroups(
  page: Page,
  read: GroupReader,
  provider: Provider,
  password: string,
) {
  await page.goto("/");
  await page.getByLabel("Username", { exact: true }).fill("admin");
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page
    .getByRole("button", { name: "Create administrator", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
  const user = await read<Schema<"User">>("/api/v1/me");
  const saved = await seedPreferencesLibrary(page, read, user);
  await page
    .getByRole("navigation", { name: "Settings sections" })
    .getByRole("link", { name: "Plugins", exact: true })
    .click();
  await page.getByRole("button", { name: /^This server/ }).click();
  await page
    .getByLabel("Service address", { exact: true })
    .fill(provider.address);
  const offered = await preferencesAction<Schema<"PluginService">>(
    page,
    page.getByRole("button", { name: "Offer service", exact: true }),
    "POST",
    "/api/v1/plugins/services",
    201,
  );
  if (!offered.body)
    throw new Error("The owned artist service offer is missing");
  expect(
    isDeepStrictEqual(offered.sent, { base_url: provider.address }) &&
      offered.body.name === provider.manifest.name,
  ).toBe(true);
  await page.getByRole("button", { name: "Your plugins", exact: true }).click();
  await page
    .getByRole("button", { name: `Connect ${offered.body.name}`, exact: true })
    .click();
  await page
    .getByLabel("Bearer token (optional)", { exact: true })
    .fill(provider.token);
  const reviewed = await reviewPluginInstallation(
    page,
    offered.body,
    provider.manifest.name,
  );
  const installed = await preferencesAction<Schema<"PluginInstallation">>(
    page,
    page.getByRole("button", { name: "Accept and connect", exact: true }),
    "POST",
    "/api/v1/plugins",
    201,
  );
  if (!installed.body)
    throw new Error("The approved artist service is missing");
  const connection = installed.body;
  expect(
    connection.credential_set &&
      connection.service_id === offered.body.id &&
      isDeepStrictEqual(connection.manifest, provider.manifest) &&
      isDeepStrictEqual(installed.sent, {
        ...reviewed.sent,
        accept_manifest_digest: reviewed.body.digest,
      }),
  ).toBe(true);
  const selected = {
    kind: "http_plugin",
    installation_id: connection.id,
  } satisfies Schema<"MetadataProvider">;
  const panel = await openPreferences(page);
  await removeMetadataProvider(panel, "MusicBrainz");
  await addMetadataProvider(panel, offered.body.name);
  await savePreferences(page, {
    revision: 0,
    enabled: true,
    providers: [selected],
  });
  await openLibrarySection(page, "Playlists");
  await openPlaylistImport(page);
  const form = page.getByRole("region", {
    name: "Import a playlist",
    exact: true,
  });
  await form
    .getByRole("combobox", { name: "Playlist service", exact: true })
    .selectOption(connection.id);
  await form
    .getByRole("textbox", { name: "Playlist link or identifier", exact: true })
    .fill(provider.reference);
  const imported = await preferencesAction<Schema<"PlaylistImportReceipt">>(
    page,
    form.getByRole("button", { name: "Import a copy", exact: true }),
    "POST",
    `/api/v1/plugins/${connection.id}/playlists/import`,
    200,
  );
  if (!imported.body) throw new Error("The owned playlist import is missing");
  const request = imported.sent as Schema<"ImportProviderPlaylist">;
  expect(
    request.playlist_reference === provider.reference &&
      request.installation_revision === connection.revision &&
      imported.body.request_id === request.request_id,
  ).toBe(true);
  const playlist = imported.body.playlist_id;
  await expect(page).toHaveURL(new RegExp(`/playlists/${playlist}$`));
  const copy = await read<Schema<"PlaylistDetail">>(
    `/api/v1/playlists/${playlist}`,
  );
  const importedIds = copy.entries
    .slice(0, 2)
    .map((entry) => entry.recording_id);
  if (!importedIds[0] || !importedIds[1])
    throw new Error("The owned imported recordings are missing");
  expect(
    copy.entries.length === 3 &&
      importedIds[0] !== importedIds[1] &&
      copy.entries[2].recording_id === importedIds[0] &&
      new Set(copy.entries.map((entry) => entry.entry_id)).size === 3 &&
      copy.entries.every((entry) => !entry.available) &&
      copy.playlist.kind === "local" &&
      copy.origin?.installation_id === connection.id &&
      copy.origin.provider === provider.manifest.id,
  ).toBe(true);
  const operation = {
    id: randomUUID(),
    action: {
      kind: "recording_artist_set",
      recording_id: importedIds[1],
      artist: saved.privateValues[1],
    },
  } satisfies Schema<"Operation">;
  const response = await page.request.post("/api/v1/sync", {
    headers: {
      "x-hito-request": "1",
      "x-hito-account": user.id,
      Origin: new URL(page.url()).origin,
    },
    data: {
      device_id: "owned-artist-groups-preparation",
      operations: [operation],
    } satisfies Schema<"SyncBatch">,
    timeout: 10_000,
  });
  try {
    const bytes = await response.body();
    if (bytes.length > 4096)
      throw new Error("Owned namesake preparation exceeded its bound");
    const receipt = JSON.parse(bytes.toString()) as Schema<"SyncReceipt">;
    expect(
      response.status() === 200 &&
        isDeepStrictEqual(receipt.acknowledged, [operation.id]),
    ).toBe(true);
  } finally {
    await response.dispose();
  }
  const ids = [...saved.ids, importedIds[0], importedIds[1]];
  const catalog = await read<Schema<"RecordingPage">>("/api/v1/recordings");
  expect(
    catalog.total === 4 &&
      new Set(ids).size === 4 &&
      catalog.items.length === 4,
  ).toBe(true);
  const members = ids.map((id, index) => {
    const recording = catalog.items.find((item) => item.id === id);
    if (!recording || !recording.release_id)
      throw new Error("An owned recording appearance is missing");
    return { recording, candidate: index === 3 ? 0 : 1 };
  });
  expect(
    new Set(members.slice(0, 3).map(({ recording }) => recording.library_id))
      .size === 2 &&
      new Set(members.slice(0, 3).map(({ recording }) => recording.release_id))
        .size === 3 &&
      members[0].recording.artist === members[3].recording.artist,
  ).toBe(true);
  return {
    user,
    local: saved,
    saved: { ...saved, ids },
    selected,
    connection,
    playlist,
    members,
  };
}

export type PreparedArtistGroups = Awaited<
  ReturnType<typeof prepareArtistGroups>
>;

export async function artistGroupsState(
  page: Page,
  read: GroupReader,
  prepared: PreparedArtistGroups,
) {
  const local = await preferencesLibraryState(page, read, prepared.local);
  const imported = prepared.saved.ids.slice(prepared.local.ids.length);
  expect(imported.length === 2 && prepared.local.ids.length === 2).toBe(true);
  const progress = await page.request.post("/api/v1/recordings/progress", {
    headers: { "x-hito-request": "1", Origin: new URL(page.url()).origin },
    data: { recording_ids: imported } satisfies Schema<"SavedPlaybackRequest">,
    timeout: 10_000,
  });
  let importedProgress: Schema<"RecordingProgress">[];
  try {
    const bytes = await progress.body();
    if (bytes.length > 524_288)
      throw new Error("Owned imported progress exceeded its bound");
    expect(progress.status()).toBe(200);
    importedProgress = JSON.parse(
      bytes.toString(),
    ) as Schema<"RecordingProgress">[];
  } finally {
    await progress.dispose();
  }
  const metadata: Schema<"RecordingMetadata">[] = [];
  const sources: Schema<"Source">[][] = [];
  const choices: { status: 200; choice: Schema<"SourceChoice"> }[] = [];
  for (const id of imported) {
    metadata.push(
      await read<Schema<"RecordingMetadata">>(
        `/api/v1/recordings/${id}/metadata`,
      ),
    );
    sources.push(
      await read<Schema<"Source">[]>(`/api/v1/recordings/${id}/sources`),
    );
    const choice = await read<Schema<"SourceChoice">>(
      `/api/v1/recordings/${id}/source-choice`,
    );
    expect(
      isDeepStrictEqual(choice, {
        recording_id: id,
        revision: 0,
        mode: "automatic",
        sources: [],
      } satisfies Schema<"SourceChoice">),
    ).toBe(true);
    choices.push({ status: 200, choice });
  }
  return {
    music: {
      ...local,
      metadata: [...local.metadata, ...metadata],
      sources: [...local.sources, ...sources],
      choices: [...local.choices, ...choices],
      progress: [...local.progress, ...importedProgress],
    },
    imported: await read<Schema<"PlaylistDetail">>(
      `/api/v1/playlists/${prepared.playlist}`,
    ),
    associations: await Promise.all(
      prepared.saved.ids.map((id) =>
        read<Schema<"ArtistAssociation">[]>(
          `/api/v1/recordings/${id}/artist-associations`,
        ),
      ),
    ),
    policy: await read<Schema<"MetadataSettings">>("/api/v1/metadata/settings"),
    plugins: await read<Schema<"PluginSettings">>("/api/v1/plugins"),
  };
}
