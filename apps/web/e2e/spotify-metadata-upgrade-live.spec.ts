import { randomUUID } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { test, type APIRequestContext } from "@playwright/test";
import {
  liveCheck,
  readSpotifyAlbumsLive,
  spotifyAlbumsLiveConfiguration,
} from "./fixtures/spotify-albums-live";
import {
  SpotifyAlbumsProjectionReader,
  verifySpotifyStoredAlbums,
} from "./fixtures/spotify-albums-projections";
import {
  freshUpgradeControl,
  signalLegacyAndWait,
  upgradeBrowserGuard,
} from "./fixtures/spotify-metadata-upgrade-control";
import {
  rememberSpotifyValues,
  spotifyUpgradeState,
  upgradeManifest,
  type UpgradeSchema,
} from "./fixtures/spotify-metadata-upgrade-state";
import {
  browseRepairedSpotifyAlbum,
  verifyLegacySpotifyState,
} from "./fixtures/spotify-metadata-upgrade-checks";
import { verifyLiveSpotifyRepair } from "./fixtures/spotify-metadata-upgrade-repair";
import { prepareSpotifyMetadataUpgrade } from "./fixtures/spotify-metadata-upgrade-setup";
import {
  findMissingSpotifyAlbums,
  replaySpotifyUpgradeReceipts,
} from "./fixtures/spotify-metadata-upgrade-actions";

test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  serviceWorkers: "block",
  launchOptions: { args: [] },
});
test.skip(
  process.env.HITO_SPOTIFY_METADATA_UPGRADE_E2E !== "1",
  "Requires a fresh isolated server, legacy plugin and operator-controlled replacement",
);

test("an existing public Spotify copy gains live albums through its original approved connection", async ({
  page,
  context,
  playwright,
  baseURL,
  browserName,
}) => {
  test.setTimeout(240_000);
  page.setDefaultTimeout(10_000);
  page.setDefaultNavigationTimeout(15_000);
  let phase = 1;
  let failed = false;
  let plugin: APIRequestContext | null = null;
  let guard: Awaited<ReturnType<typeof upgradeBrowserGuard>> | null = null;
  const privateValues = new Set<string>();
  const password = `owned-disposable-upgrade-${randomUUID()}`;
  privateValues.add(password);
  const publicApi = new SpotifyAlbumsProjectionReader(page.request);
  const read = publicApi.read;
  const evidence = {
    recordings: 0,
    entries: 0,
    updated: 0,
    unchanged: 0,
    unavailable: 0,
    albums: 0,
    manifest_equal: false,
    current_snapshot_verified: false,
    receipt_replays: 0,
    privacy_passed: false,
  };
  try {
    liveCheck(
      browserName === "chromium" && baseURL === "http://127.0.0.1:8080",
    );
    liveCheck(
      (await read<UpgradeSchema["SetupStatus"]>("/api/v1/setup")).needs_setup,
    );
    await freshUpgradeControl();
    guard = await upgradeBrowserGuard(context, page, privateValues);
    const configuration = spotifyAlbumsLiveConfiguration();
    privateValues.add(configuration.reference);
    privateValues.add(encodeURIComponent(configuration.reference));
    privateValues.add(configuration.pluginAddress);
    plugin = await playwright.request.newContext({
      baseURL: configuration.pluginAddress,
      timeout: 15_000,
      extraHTTPHeaders: { accept: "application/json" },
    });
    const originalManifest = await upgradeManifest(plugin);
    phase = 2;
    const legacy = await readSpotifyAlbumsLive(
      plugin,
      configuration.playlistId,
      configuration.reference,
      "legacy",
    );
    rememberSpotifyValues(privateValues, legacy);
    phase = 3;
    const prepared = await prepareSpotifyMetadataUpgrade(
      page,
      read,
      legacy,
      originalManifest.manifest,
      configuration.pluginAddress,
      configuration.reference,
      password,
    );
    for (const value of Object.values(prepared.personal))
      privateValues.add(value);
    for (const cookie of await context.cookies())
      if (cookie.value) privateValues.add(cookie.value);
    const before = await spotifyUpgradeState(read, prepared);
    verifyLegacySpotifyState(before, prepared);
    const recordingIds = [...prepared.mapped.recordings.values()]
      .map((item) => item.recording_id)
      .sort();
    liveCheck(
      recordingIds.length === new Set(recordingIds).size &&
        recordingIds.every((id) => /^[0-9a-f-]{36}$/.test(id)),
    );
    evidence.recordings = recordingIds.length;
    evidence.entries = before.copy.entries.length;
    guard.seal();
    phase = 4;
    await signalLegacyAndWait({
      version: 1,
      account_id: prepared.user.id,
      playlist_id: prepared.playlist,
      installation_id: prepared.connection.id,
      service_id: prepared.service.id,
      installation_revision: prepared.connection.revision,
      service_revision: prepared.service.revision,
      recording_ids: recordingIds,
    });
    phase = 5;
    const manifest = await upgradeManifest(plugin);
    evidence.manifest_equal = manifest.bytes.equals(originalManifest.bytes);
    liveCheck(
      evidence.manifest_equal &&
        isDeepStrictEqual(manifest.manifest, originalManifest.manifest) &&
        isDeepStrictEqual(
          await read<UpgradeSchema["PluginSettings"]>("/api/v1/plugins"),
          before.settings,
        ) &&
        isDeepStrictEqual(
          await read<UpgradeSchema["User"]>("/api/v1/me"),
          before.user,
        ),
    );
    const current = await readSpotifyAlbumsLive(
      plugin,
      configuration.playlistId,
      configuration.reference,
    );
    rememberSpotifyValues(privateValues, current);
    liveCheck(
      current.recordings.get(prepared.selectedProvider)?.release != null,
    );
    phase = 6;
    const repaired = await findMissingSpotifyAlbums(page, prepared);
    const receipt = repaired.body;
    phase = 7;
    const after = await spotifyUpgradeState(read, prepared);
    const verified = verifyLiveSpotifyRepair(
      before,
      after,
      prepared,
      current,
      receipt,
    );
    evidence.current_snapshot_verified = true;
    evidence.updated = verified.updated;
    evidence.unchanged = verified.unchanged;
    evidence.unavailable = verified.unavailable;
    const albums = await verifySpotifyStoredAlbums(
      read,
      verified.published,
      verified.releases,
    );
    evidence.albums = albums.length;
    phase = 8;
    evidence.receipt_replays = await replaySpotifyUpgradeReceipts(
      page,
      prepared,
      repaired,
    );
    liveCheck(
      isDeepStrictEqual(await spotifyUpgradeState(read, prepared), after),
    );
    phase = 9;
    await browseRepairedSpotifyAlbum(
      page,
      read,
      albums,
      after,
      verified.published,
    );
    phase = 10;
  } catch {
    failed = true;
  } finally {
    try {
      await page.goto("about:blank", { timeout: 5000 });
    } catch {
      failed = true;
    }
    try {
      await plugin?.dispose();
    } catch {
      failed = true;
    }
    try {
      liveCheck(guard);
      await guard.verify();
      evidence.privacy_passed = true;
    } catch {
      failed = true;
    }
  }
  console.info(
    JSON.stringify({
      event: failed
        ? "spotify_metadata_upgrade_live_failed"
        : "spotify_metadata_upgrade_live_verified",
      phase,
      ...evidence,
      browser_requests: guard?.counts.requests ?? 0,
      public_reads: publicApi.requests,
      ui_imports: guard?.counts.imports ?? 0,
      ui_repairs: guard?.counts.repairs ?? 0,
      forbidden: guard?.counts.forbidden ?? 0,
      page_errors: guard?.counts.page_errors ?? 0,
    }),
  );
  if (failed)
    throw new Error(
      `Live Spotify metadata upgrade failed at phase ${phase}; the live comparison was not proven`,
    );
});
