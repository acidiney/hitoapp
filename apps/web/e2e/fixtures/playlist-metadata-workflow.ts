import { randomUUID } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { expect, type Page } from "@playwright/test";
import type { components } from "../../src/generated";
import { reviewPluginInstallation } from "./plugin-installation-review";
import {
  creditAction,
  creditPost,
  releaseCreditReader,
} from "./release-credits";
import type { playlistMetadataProvider } from "./playlist-metadata-provider";
import { openLibrarySection } from "./library-sections";

type Schema = components["schemas"];
type Reader = ReturnType<typeof releaseCreditReader>;
type Provider = Awaited<ReturnType<typeof playlistMetadataProvider>>;

export async function prepareMetadataCopy(
  page: Page,
  owned: Provider,
  password: string,
  read: Reader,
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
  const user = await read<Schema["User"]>("/api/v1/me");
  const navigation = page.getByRole("navigation", { name: "Main navigation" });
  await page
    .getByRole("link", { name: "Conta e definições", exact: true })
    .click();
  await page.getByRole("tab", { name: "Plugins", exact: true }).click();
  await page.getByText("Services for this server", { exact: true }).click();
  await page.getByLabel("Service address", { exact: true }).fill(owned.address);
  const offered = await creditPost<Schema["PluginService"]>(
    page,
    "/api/v1/plugins/services",
    201,
    () =>
      page.getByRole("button", { name: "Offer service", exact: true }).click(),
  );
  expect(
    offered.body.base_url === owned.address &&
      offered.body.name === owned.manifest.name,
  ).toBe(true);
  await page
    .getByRole("combobox", { name: "Service", exact: true })
    .selectOption(offered.body.id);
  await page
    .getByLabel("Bearer token (optional)", { exact: true })
    .fill(owned.token);
  const reviewed = await reviewPluginInstallation(
    page,
    offered.body,
    owned.manifest.name,
  );
  const installed = await creditPost<Schema["PluginInstallation"]>(
    page,
    "/api/v1/plugins",
    201,
    () =>
      page
        .getByRole("button", { name: "Accept and connect", exact: true })
        .click(),
  );
  const connection = installed.body;
  expect(
    connection.credential_set &&
      connection.service_id === offered.body.id &&
      connection.manifest.capabilities.join(",") === "playlists" &&
      isDeepStrictEqual(installed.sent, {
        ...reviewed.sent,
        accept_manifest_digest: reviewed.body.digest,
      }),
  ).toBe(true);
  await openLibrarySection(page, "Playlists");
  const form = page.getByRole("region", { name: "Import a playlist" });
  await form
    .getByRole("combobox", { name: "Playlist service" })
    .selectOption(connection.id);
  await form
    .getByRole("textbox", { name: "Playlist link or identifier" })
    .fill(owned.reference);
  const imported = await creditPost<Schema["PlaylistImportReceipt"]>(
    page,
    `/api/v1/plugins/${connection.id}/playlists/import`,
    200,
    () =>
      form.getByRole("button", { name: "Import a copy", exact: true }).click(),
  );
  const importRequest = imported.sent as Schema["ImportProviderPlaylist"];
  expect(
    importRequest.playlist_reference === owned.reference &&
      importRequest.installation_revision === connection.revision &&
      imported.body.request_id === importRequest.request_id,
  ).toBe(true);
  const playlist = imported.body.playlist_id;
  await expect(page).toHaveURL(new RegExp(`/playlists/${playlist}$`));
  const copy = await read<Schema["PlaylistDetail"]>(
    `/api/v1/playlists/${playlist}`,
  );
  const first = copy.entries[1]?.recording_id;
  const second = copy.entries[0]?.recording_id;
  if (!first || !second)
    throw new Error("The imported metadata identities are missing");
  expect(
    first !== second &&
      copy.entries.length === 5 &&
      copy.entries[2].recording_id === second &&
      copy.entries[3].recording_id === null &&
      copy.entries[4].recording_id === null &&
      new Set(copy.entries.map((entry) => entry.entry_id)).size === 5 &&
      copy.entries.every((entry) => !entry.available) &&
      copy.playlist.kind === "local" &&
      copy.origin?.installation_id === connection.id &&
      copy.origin.provider === owned.manifest.id &&
      isDeepStrictEqual(
        copy.entries.map((entry) => entry.origin?.kind),
        ["recording", "recording", "recording", "unavailable", "unknown"],
      ),
  ).toBe(true);
  const table = page.getByRole("table", {
    name: "Playlist entries",
    exact: true,
  });
  await expect(table.locator("tbody tr")).toHaveCount(5);
  await table
    .getByRole("link", {
      name: `Details for ${owned.recordings[0].title}`,
      exact: true,
    })
    .click();
  const editor = page.getByRole("region", {
    name: "Your track details",
    exact: true,
  });
  const personal = {
    title: `${owned.prefix} personal title`,
    artist: `${owned.prefix} personal performer`,
  };
  for (const field of ["title", "artist"] as const) {
    await editor
      .getByRole("textbox", { name: `Track ${field}`, exact: true })
      .fill(personal[field]);
    await creditAction(
      page,
      field === "title"
        ? {
            kind: "recording_title_set",
            recording_id: first,
            title: personal.title,
          }
        : {
            kind: "recording_artist_set",
            recording_id: first,
            artist: personal.artist,
          },
      () =>
        editor
          .getByRole("button", { name: `Save personal ${field}`, exact: true })
          .click(),
    );
    await expect(
      editor.getByRole("button", { name: `Use source ${field}`, exact: true }),
    ).toBeVisible();
  }
  await page.goto(`/playlists/${playlist}`);
  await creditAction(
    page,
    { kind: "like_set", recording_id: first, liked: true },
    () =>
      table
        .getByRole("button", { name: `Like ${personal.title}`, exact: true })
        .click(),
  );
  await expect(
    table.getByRole("button", {
      name: `Unlike ${personal.title}`,
      exact: true,
    }),
  ).toBeVisible();
  const operations = [
    {
      kind: "progress_set",
      recording_id: first,
      position_ms: 4_200,
      speed: 1.25,
    },
    {
      kind: "playlist_rename",
      playlist_id: playlist,
      name: `${owned.prefix} personal selection`,
    },
    {
      kind: "entry_move",
      playlist_id: playlist,
      entry_id: copy.entries[1].entry_id,
      position: -10,
    },
  ] satisfies Schema["Action"][];
  const batch = {
    device_id: "owned-metadata-browser",
    operations: operations.map((action) => ({ id: randomUUID(), action })),
  } satisfies Schema["SyncBatch"];
  const response = await page.request.post("/api/v1/sync", {
    headers: { "x-hito-request": "1", "x-hito-account": user.id },
    data: batch,
    timeout: 10_000,
  });
  expect(response.status()).toBe(200);
  const receipt = (await response.json()) as Schema["SyncReceipt"];
  expect(
    isDeepStrictEqual(
      receipt.acknowledged,
      batch.operations.map((operation) => operation.id),
    ),
  ).toBe(true);
  await page.reload();
  await expect(
    page.getByRole("heading", {
      name: `${owned.prefix} personal selection`,
      exact: true,
    }),
  ).toBeVisible();
  return {
    user,
    connection,
    playlist,
    ids: [first, second],
    importRequest,
    importReceipt: imported.body,
    personal,
  };
}

export async function metadataState(
  read: Reader,
  playlist: string,
  ids: string[],
) {
  return {
    copy: await read<Schema["PlaylistDetail"]>(`/api/v1/playlists/${playlist}`),
    catalog: await read<Schema["RecordingPage"]>("/api/v1/recordings"),
    albums: await read<Schema["ReleasePage"]>("/api/v1/releases"),
    activity: await read<Schema["Activity"]>("/api/v1/activity"),
    events: await read<Schema["SyncPage"]>("/api/v1/sync?cursor=0"),
    metadata: await Promise.all(
      ids.map((id) =>
        read<Schema["RecordingMetadata"]>(`/api/v1/recordings/${id}/metadata`),
      ),
    ),
    sources: await Promise.all(
      ids.map((id) =>
        read<Schema["Source"][]>(`/api/v1/recordings/${id}/sources`),
      ),
    ),
    choices: await Promise.all(
      ids.map((id) =>
        read<Schema["SourceChoice"]>(`/api/v1/recordings/${id}/source-choice`),
      ),
    ),
  };
}

export function verifyMetadataRepair(
  before: Awaited<ReturnType<typeof metadataState>>,
  after: Awaited<ReturnType<typeof metadataState>>,
  ids: string[],
  owned: Provider,
) {
  const copyIdentity = (copy: Schema["PlaylistDetail"]) => ({
    playlist: copy.playlist,
    origin: copy.origin,
    entries: copy.entries.map((entry) => [
      entry.entry_id,
      entry.recording_id,
      entry.position,
      entry.origin,
    ]),
  });
  expect(
    isDeepStrictEqual(copyIdentity(after.copy), copyIdentity(before.copy)),
  ).toBe(true);
  expect(
    before.catalog.total === 2 &&
      before.albums.total === 0 &&
      after.catalog.total === 2 &&
      after.albums.total === 1,
  ).toBe(true);
  expect(
    isDeepStrictEqual(after.sources, before.sources) &&
      isDeepStrictEqual(after.choices, before.choices) &&
      after.sources.every((sources) => sources.length === 0),
  ).toBe(true);
  const progress = (state: typeof before) =>
    state.activity.history.map((entry) => [
      entry.id,
      entry.position_ms,
      entry.speed,
      entry.updated_at,
      entry.liked,
    ]);
  expect(isDeepStrictEqual(progress(after), progress(before))).toBe(true);
  expect(after.catalog.items.find((item) => item.id === ids[0])?.liked).toBe(
    true,
  );
  for (const [index, metadata] of after.metadata.entries()) {
    const previous = before.metadata[index];
    for (const field of ["title", "artist"] as const)
      for (const retained of ["personal", "operation_id", "value"] as const)
        if (previous[field].personal !== null)
          expect(metadata[field][retained] === previous[field][retained]).toBe(
            true,
          );
    expect(
      previous.selected_observation?.release_id === null &&
        metadata.selected_observation?.release_id ===
          after.albums.items[0].id &&
        metadata.observations.items.length ===
          previous.observations.items.length + 1 &&
        previous.observations.items.every((old) =>
          metadata.observations.items.some((item) =>
            isDeepStrictEqual(item, old),
          ),
        ) &&
        metadata.selected_observation?.source_id === null &&
        metadata.selected_observation.album_artist === null &&
        metadata.selected_observation.provider_recording_id ===
          owned.recordings[index].id,
    ).toBe(true);
  }
  const old = before.events.operations;
  expect(
    after.events.operations.length === old.length + 1 &&
      isDeepStrictEqual(after.events.operations.slice(0, old.length), old),
  ).toBe(true);
  const event = after.events.operations.at(-1);
  if (
    !event ||
    !("publication" in event) ||
    event.publication.kind !== "playlist_metadata_refreshed"
  )
    throw new Error("A metadata-only publication is required");
  expect(
    event.publication.playlist_id === before.copy.playlist.id &&
      event.publication.recordings.length === 2 &&
      isDeepStrictEqual(
        new Set(
          event.publication.recordings.map(
            (recording) => recording.recording_id,
          ),
        ),
        new Set(ids),
      ) &&
      event.publication.origin.installation_id ===
        before.copy.origin?.installation_id &&
      event.publication.origin.provider === owned.manifest.id &&
      event.publication.origin.provider_playlist_id === owned.playlist.id &&
      event.publication.origin.snapshot === "verified-one" &&
      !Object.hasOwn(event.publication, "entries") &&
      !Object.hasOwn(event.publication, "name"),
  ).toBe(true);
  const album = after.albums.items[0];
  expect(
    album.title === owned.album.title &&
      album.artist === null &&
      album.artist_id === null &&
      album.track_count === 2,
  ).toBe(true);
  return album.id;
}
