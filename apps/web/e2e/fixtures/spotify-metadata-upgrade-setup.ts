import { isDeepStrictEqual } from "node:util";
import type { Page } from "@playwright/test";
import { reviewPluginInstallation } from "./plugin-installation-review";
import { creditPost } from "./release-credits";
import {
  liveCheck,
  verifySpotifyAlbumsPublication,
} from "./spotify-albums-live";
import { verifySpotifyStoredRecordings } from "./spotify-albums-projections";
import {
  upgradeEvents,
  upgradePost,
  type UpgradeLive,
  type UpgradeRead,
  type UpgradeSchema,
} from "./spotify-metadata-upgrade-state";
import { editSpotifyUpgradeCopy } from "./spotify-metadata-upgrade-actions";
import { openLibrarySection } from "./library-sections";

export async function prepareSpotifyMetadataUpgrade(
  page: Page,
  read: UpgradeRead,
  legacy: UpgradeLive,
  manifest: UpgradeSchema["Manifest"],
  address: string,
  reference: string,
  password: string,
) {
  await page.goto("/");
  await page.getByLabel("Username", { exact: true }).fill("admin");
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page
    .getByRole("button", { name: "Create administrator", exact: true })
    .click();
  await page
    .getByRole("heading", { name: "Your library", exact: true })
    .waitFor();
  const user = await read<UpgradeSchema["User"]>("/api/v1/me");
  const navigation = page.getByRole("navigation", { name: "Main navigation" });
  await page
    .getByRole("link", { name: "Account and settings", exact: true })
    .click();
  await page
    .getByRole("navigation", { name: "Settings sections" })
    .getByRole("link", { name: "Plugins", exact: true })
    .click();
  await page.getByText("Services for this server", { exact: true }).click();
  await page.getByLabel("Service address", { exact: true }).fill(address);
  const offered = await creditPost<UpgradeSchema["PluginService"]>(
    page,
    "/api/v1/plugins/services",
    201,
    () =>
      page.getByRole("button", { name: "Offer service", exact: true }).click(),
  );
  const service = offered.body;
  liveCheck(
    service.base_url === address &&
      service.name === manifest.name &&
      isDeepStrictEqual(offered.sent, { base_url: address }),
  );
  await page
    .getByRole("combobox", { name: "Service", exact: true })
    .selectOption(service.id);
  liveCheck(
    (await page
      .getByLabel("Bearer token (optional)", { exact: true })
      .inputValue()) === "",
  );
  const reviewed = await reviewPluginInstallation(page, service, manifest.name);
  const installed = await creditPost<UpgradeSchema["PluginInstallation"]>(
    page,
    "/api/v1/plugins",
    201,
    () =>
      page
        .getByRole("button", { name: "Accept and connect", exact: true })
        .click(),
  );
  const connection = installed.body;
  liveCheck(
    !connection.credential_set &&
      connection.service_id === service.id &&
      isDeepStrictEqual(connection.manifest, manifest) &&
      isDeepStrictEqual(installed.sent, {
        service_id: service.id,
        service_revision: service.revision,
        bearer_token: null,
        accept_manifest_digest: reviewed.body.digest,
      }),
  );
  await openLibrarySection(page, "Playlists");
  const form = page.getByRole("region", {
    name: "Import a playlist",
    exact: true,
  });
  await form.getByRole("radio", { name: "Import a copy", exact: true }).check();
  await form
    .getByRole("combobox", { name: "Playlist service", exact: true })
    .selectOption(connection.id);
  await form
    .getByRole("textbox", { name: "Playlist link or identifier", exact: true })
    .fill(reference);
  const imported = await upgradePost<UpgradeSchema["PlaylistImportReceipt"]>(
    page,
    `/api/v1/plugins/${connection.id}/playlists/import`,
    () =>
      form.getByRole("button", { name: "Import a copy", exact: true }).click(),
  );
  const importRequest =
    imported.sent as UpgradeSchema["ImportProviderPlaylist"];
  liveCheck(
    imported.body.request_id === importRequest.request_id &&
      importRequest.installation_revision === connection.revision &&
      importRequest.playlist_reference === reference,
  );
  const playlist = imported.body.playlist_id;
  await page.waitForURL(`http://127.0.0.1:8080/playlists/${playlist}`);
  const original = await read<UpgradeSchema["PlaylistDetail"]>(
    `/api/v1/playlists/${playlist}`,
  );
  const initialEvents = await upgradeEvents(read);
  liveCheck(initialEvents.length === 1);
  const event = initialEvents[0];
  liveCheck(
    "publication" in event && event.publication.kind === "playlist_imported",
  );
  const mapped = verifySpotifyAlbumsPublication(
    legacy,
    original,
    event.publication.snapshot,
    connection,
    service,
  );
  liveCheck(mapped.recordings.size >= 2 && mapped.releases.size === 0);
  const publications = new Map(
    [...mapped.recordings.values()].map((item) => [item.recording_id, item]),
  );
  await verifySpotifyStoredRecordings(read, publications, publications.size);
  const entry = original.entries.find(
    (item) =>
      item.recording_id &&
      item.origin?.kind === "recording" &&
      item.duration_ms >= 10_000,
  );
  liveCheck(entry?.recording_id && entry.origin?.provider_recording_id);
  const selected = entry.recording_id;
  const edited = await editSpotifyUpgradeCopy(
    page,
    user.id,
    playlist,
    selected,
    entry.entry_id,
    original.entries.length,
  );
  await page.reload();
  await page
    .getByRole("heading", { name: edited.personal.playlist, exact: true })
    .waitFor();
  return {
    user,
    service,
    connection,
    playlist,
    selected,
    selectedProvider: entry.origin.provider_recording_id,
    movedEntry: entry.entry_id,
    original,
    importEvent: event,
    ...edited,
    imported,
    importRequest,
    mapped,
  };
}

export type PreparedSpotifyUpgrade = Awaited<
  ReturnType<typeof prepareSpotifyMetadataUpgrade>
>;
