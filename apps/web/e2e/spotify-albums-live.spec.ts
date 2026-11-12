import { isDeepStrictEqual } from "node:util";
import { openPlaylistImport } from "./fixtures/playlist-dialogs";
import { expect, test, type APIRequestContext } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import { reviewPluginInstallation } from "./fixtures/plugin-installation-review";
import { captureConsolePrivacy } from "./fixtures/console-privacy";
import { creditPost } from "./fixtures/release-credits";
import {
  liveCheck,
  readSpotifyAlbumsLive,
  spotifyAlbumsLiveConfiguration,
  verifySpotifyAlbumsPublication,
} from "./fixtures/spotify-albums-live";
import {
  SpotifyAlbumsProjectionReader,
  spotifyAlbumEditionChecks,
  verifySpotifyAlbumCredit,
  verifySpotifyAlbumEdition,
  verifySpotifyPerformerCredit,
  verifySpotifyStoredAlbums,
  verifySpotifyStoredArtists,
  verifySpotifyStoredRecordings,
  verifySpotifyTrackCredit,
} from "./fixtures/spotify-albums-projections";
import { openLibrarySection } from "./fixtures/library-sections";

test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  serviceWorkers: "block",
  launchOptions: { args: [] },
});
test.skip(
  process.env.HITO_SPOTIFY_ALBUMS_LIVE_E2E !== "1",
  "Requires an explicitly disposable server and opted-in public Spotify playlist",
);

test("live public Spotify metadata becomes stored albums with honest private credits", async ({
  page,
  context,
  playwright,
  baseURL,
  browserName,
}, info) => {
  test.setTimeout(240_000);
  page.setDefaultTimeout(10_000);
  page.setDefaultNavigationTimeout(15_000);
  let phase = 1;
  let phase8Step = 0;
  let editionChecks: ReturnType<typeof spotifyAlbumEditionChecks> | null = null;
  let failed = false;
  let plugin: APIRequestContext | null = null;
  const publicApi = new SpotifyAlbumsProjectionReader(page.request);
  const read = publicApi.read;
  let requests = 0;
  let imports = 0;
  let foreignRequests = 0;
  let audioRequests = 0;
  let forbiddenWrites = 0;
  let downloads = 0;
  let extraPages = 0;
  let pageErrors = 0;
  let privacyFailed = false;
  let consoleBytes = 0;
  let verifiedEntries = 0;
  let verifiedRecordings = 0;
  let verifiedAlbums = 0;
  let verifiedArtists = 0;
  let duplicateEntries = 0;
  let upstreamPages = 0;
  const pending = new Set<Promise<void>>();
  const captured: string[] = [];
  const privateValues = new Set<string>();
  const password = "disposable live album browser password";
  privateValues.add(password);
  await captureConsolePrivacy(
    context,
    (text) => {
      consoleBytes += Buffer.byteLength(text);
      if (consoleBytes <= 524_288) captured.push(text);
      else privacyFailed = true;
    },
    () => {
      privacyFailed = true;
    },
    pending,
  );
  page.on("pageerror", () => pageErrors++);
  page.on("download", () => downloads++);
  context.on("page", (extra) => {
    extraPages++;
    void extra.close().catch(() => {
      privacyFailed = true;
    });
  });
  try {
    liveCheck(baseURL === "http://127.0.0.1:8080");
    const origin = new URL(baseURL).origin;
    liveCheck((await read<Schema<"SetupStatus">>("/api/v1/setup")).needs_setup);
    const configuration = spotifyAlbumsLiveConfiguration();
    privateValues.add(configuration.reference);
    privateValues.add(encodeURIComponent(configuration.reference));
    privateValues.add(configuration.pluginAddress);
    await context.route("**/*", async (route) => {
      const request = route.request();
      const target = new URL(request.url());
      requests++;
      const foreign = target.origin !== origin;
      const audio =
        request.resourceType() === "media" ||
        /\/audio(?:\/|$)/.test(target.pathname);
      const oauth = /\/(?:oauth|authorization)(?:\/|$)/.test(target.pathname);
      const write = request.method() !== "GET" && request.method() !== "HEAD";
      const forbidden =
        oauth ||
        (write &&
          /\/(?:download|sources|source-choice|search|sync)(?:\/|$)/.test(
            target.pathname,
          ));
      if (foreign) foreignRequests++;
      if (audio) audioRequests++;
      if (forbidden) forbiddenWrites++;
      if (write && /\/playlists\/import$/.test(target.pathname)) imports++;
      if (foreign || audio || forbidden || requests > 2000) await route.abort();
      else await route.continue();
    });

    phase = 2;
    plugin = await playwright.request.newContext({
      baseURL: configuration.pluginAddress,
      timeout: 15_000,
      extraHTTPHeaders: { accept: "application/json" },
    });
    const expected = await readSpotifyAlbumsLive(
      plugin,
      configuration.playlistId,
      configuration.reference,
    );
    upstreamPages = expected.pages;
    for (const recording of expected.recordings.values()) {
      for (const value of [
        recording.id,
        recording.title,
        recording.artist,
        recording.revision,
      ])
        if (value) privateValues.add(value);
      if (recording.release) {
        for (const value of [
          recording.release.id,
          recording.release.title,
          recording.release.artist,
        ])
          if (value) privateValues.add(value);
      }
    }
    privateValues.add(expected.detail.id);
    privateValues.add(expected.detail.name);
    privateValues.add(expected.detail.snapshot);

    phase = 3;
    await page.goto("/");
    await page
      .getByRole("button", { name: "Create administrator", exact: true })
      .waitFor();
    await page.getByLabel("Username", { exact: true }).fill("admin");
    await page.getByLabel("Password", { exact: true }).fill(password);
    await page
      .getByRole("button", { name: "Create administrator", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    liveCheck(
      (await read<Schema<"Playlist">[]>("/api/v1/playlists")).length === 0,
    );
    liveCheck(
      (await read<Schema<"RecordingPage">>("/api/v1/recordings")).total === 0,
    );
    liveCheck(
      (await read<Schema<"ReleasePage">>("/api/v1/releases")).total === 0,
    );
    const navigation = page.getByRole("navigation", {
      name: "Main navigation",
    });

    phase = 4;
    await page
      .getByRole("link", { name: "Account and settings", exact: true })
      .click();
    await page.goto("/settings?section=plugins");
    await page.getByRole("button", { name: /^This server/ }).click();
    await expect(page.getByLabel("Service name", { exact: true })).toHaveCount(
      0,
    );
    await page
      .getByLabel("Service address", { exact: true })
      .fill(configuration.pluginAddress);
    const offered = await creditPost<Schema<"PluginService">>(
      page,
      "/api/v1/plugins/services",
      201,
      () =>
        page
          .getByRole("button", { name: "Offer service", exact: true })
          .click(),
    );
    const service = offered.body;
    liveCheck(
      isDeepStrictEqual(offered.sent, {
        base_url: configuration.pluginAddress,
      }),
    );
    liveCheck(
      service.base_url === configuration.pluginAddress &&
        service.name === expected.manifestName,
    );
    await page
      .getByRole("button", { name: "Your plugins", exact: true })
      .click();
    await page
      .getByRole("button", { name: `Connect ${service.name}`, exact: true })
      .click();
    liveCheck(
      (await page
        .getByLabel("Bearer token (optional)", { exact: true })
        .inputValue()) === "",
    );
    const reviewed = await reviewPluginInstallation(
      page,
      service,
      expected.manifestName,
    );
    const installed = await creditPost<Schema<"PluginInstallation">>(
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
      isDeepStrictEqual(installed.sent, {
        service_id: service.id,
        service_revision: service.revision,
        bearer_token: null,
        accept_manifest_digest: reviewed.body.digest,
      }),
    );
    liveCheck(
      connection.service_id === service.id &&
        !connection.credential_set &&
        connection.manifest.id === "org.hito.spotify" &&
        connection.manifest.name === expected.manifestName &&
        connection.manifest.authorization == null &&
        connection.manifest.capabilities.includes("playlists"),
    );

    phase = 5;
    await openLibrarySection(page, "Playlists");
    await openPlaylistImport(page);
    const form = page.getByRole("region", {
      name: "Import a playlist",
      exact: true,
    });
    await form.getByRole("radio", { name: /^Copy/ }).check();
    await form
      .getByRole("textbox", {
        name: "Playlist link or identifier",
        exact: true,
      })
      .fill(configuration.reference);
    const [imported] = await Promise.all([
      page.waitForResponse(
        (response) =>
          new URL(response.url()).pathname ===
            `/api/v1/plugins/${connection.id}/playlists/import` &&
          response.request().method() === "POST",
        { timeout: 75_000 },
      ),
      form.getByRole("button", { name: "Import a copy", exact: true }).click(),
    ]);
    liveCheck(imported.status() === 200);
    const sent = imported
      .request()
      .postDataJSON() as Schema<"ImportProviderPlaylist">;
    const receipt = (await imported.json()) as Schema<"PlaylistImportReceipt">;
    liveCheck(
      receipt.request_id === sent.request_id &&
        sent.installation_revision === connection.revision &&
        sent.playlist_reference === configuration.reference &&
        /^[0-9a-f-]{36}$/.test(sent.request_id),
    );
    await page.waitForURL(`${origin}/playlists/${receipt.playlist_id}`);
    liveCheck(
      (await page.getByRole("heading", { level: 1 }).textContent()) ===
        expected.detail.name,
    );

    phase = 6;
    const playlist = await read<Schema<"PlaylistDetail">>(
      `/api/v1/playlists/${receipt.playlist_id}`,
    );
    const feed = await read<Schema<"SyncPage">>("/api/v1/sync?cursor=0");
    liveCheck(!feed.has_more);
    const publications = feed.operations.flatMap((event) =>
      "publication" in event && event.publication.kind === "playlist_imported"
        ? [event.publication.snapshot]
        : [],
    );
    liveCheck(publications.length === 1);
    const mapped = verifySpotifyAlbumsPublication(
      expected,
      playlist,
      publications[0],
      connection,
      service,
    );
    verifiedEntries = playlist.entries.length;
    verifiedRecordings = mapped.recordings.size;
    duplicateEntries =
      expected.entries.filter((entry) => entry.kind === "recording").length -
      verifiedRecordings;

    phase = 7;
    const byRecording = new Map(
      [...mapped.recordings.values()].map((item) => [item.recording_id, item]),
    );
    const catalog = await verifySpotifyStoredRecordings(
      read,
      byRecording,
      verifiedRecordings,
    );
    const albums = await verifySpotifyStoredAlbums(
      read,
      byRecording,
      mapped.releases,
    );
    verifiedAlbums = albums.length;
    const artists = await verifySpotifyStoredArtists(
      read,
      expected.recordings,
      catalog,
    );
    verifiedArtists = artists.length;

    phase = 8;
    phase8Step = 1;
    const album = albums[0];
    liveCheck(album);
    const albumPath = `/albums/${album.id}`;
    phase8Step = 2;
    const detail = await read<Schema<"ReleaseDetail">>(
      `/api/v1/releases/${album.id}`,
    );
    phase8Step = 3;
    editionChecks = spotifyAlbumEditionChecks(detail, album, byRecording);
    verifySpotifyAlbumEdition(detail, album, byRecording);
    phase8Step = 4;
    await openLibrarySection(page, "Albums");
    const card = page
      .locator(".album-card")
      .filter({ has: page.locator(`a[href="${albumPath}"]`) });
    phase8Step = 5;
    await card.waitFor();
    liveCheck((await card.locator("h2").textContent()) === album.title);
    phase8Step = 6;
    if (album.artist === null) {
      await expect(
        card.getByText("Album artist not provided", { exact: true }),
      ).toBeVisible();
      await expect(card.locator('a[href^="/artists/"]')).toHaveCount(0);
    } else
      liveCheck(
        (await card.locator(".credit-link").textContent()) === album.artist,
      );
    phase8Step = 7;
    await card.locator(`a[href="${albumPath}"]`).first().click();
    phase8Step = 8;
    await expect
      .poll(
        async () =>
          (await page.getByRole("heading", { level: 1 }).textContent()) ===
          album.title,
        { timeout: 8_000 },
      )
      .toBe(true);
    phase8Step = 9;
    if (album.artist === null) {
      await expect(
        page.getByText("Album artist not provided", { exact: true }),
      ).toBeVisible();
      await expect(page.locator(".album-hero .credit-link")).toHaveCount(0);
    } else
      liveCheck(
        (await page.locator(".album-hero .credit-link").textContent()) ===
          album.artist,
      );
    const rows = page
      .getByRole("table", { name: "Tracks", exact: true })
      .locator("tbody tr");
    phase8Step = 10;
    await expect(rows).toHaveCount(detail.tracks.items.length);
    phase8Step = 11;
    liveCheck(
      isDeepStrictEqual(
        await rows.locator(".artist-column").allTextContents(),
        detail.tracks.items.map((track) => track.artist),
      ),
    );
    phase8Step = 12;
    liveCheck(
      isDeepStrictEqual(
        await rows.locator(".album-column").allTextContents(),
        detail.tracks.items.map(() => album.title),
      ),
    );
    phase8Step = 13;
    await page
      .getByRole("link", { name: "Edit album details", exact: true })
      .click();
    phase8Step = 14;
    liveCheck(
      (await page.getByLabel("Album artist", { exact: true }).inputValue()) ===
        (album.artist ?? ""),
    );
    phase8Step = 15;
    const releaseMetadata = await read<Schema<"ReleaseMetadata">>(
      `/api/v1/releases/${album.id}/metadata`,
    );
    phase8Step = 16;
    verifySpotifyAlbumCredit(releaseMetadata, album);
    phase8Step = 17;
    await page
      .getByRole("link", { name: "Back to album", exact: true })
      .click();
    phase8Step = 18;

    phase = 9;
    const selected = detail.tracks.items[0];
    const provider = byRecording.get(selected.id);
    liveCheck(provider);
    await rows.first().locator(`a[href="/recordings/${selected.id}"]`).click();
    liveCheck(
      (await page.getByLabel("Track title", { exact: true }).inputValue()) ===
        provider.metadata.title &&
        (await page
          .getByLabel("Track artist", { exact: true })
          .inputValue()) === provider.metadata.artist,
    );
    const metadata = await read<Schema<"RecordingMetadata">>(
      `/api/v1/recordings/${selected.id}/metadata`,
    );
    verifySpotifyTrackCredit(metadata, provider, album, connection);
    liveCheck(
      (
        await read<Schema<"Source">[]>(
          `/api/v1/recordings/${selected.id}/sources`,
        )
      ).length === 0,
    );
    const download = await page.request.get(
      `/api/v1/recordings/${selected.id}/download`,
      { timeout: 10_000, maxRedirects: 0 },
    );
    try {
      liveCheck(download.status() === 404);
    } finally {
      await download.dispose();
    }
    liveCheck(
      await page
        .locator(".metadata-observations")
        .evaluate(
          (element, expected) =>
            element.textContent?.includes(expected.title) === true &&
            element.textContent.includes(expected.artist) &&
            element.textContent.includes(expected.album),
          {
            title: provider.metadata.title,
            artist: provider.metadata.artist,
            album: album.title,
          },
        ),
    );

    phase = 10;
    const performer = artists[0];
    liveCheck(performer);
    await openLibrarySection(page, "Artists");
    await page.locator(`a[href="/artists/${performer.id}"]`).click();
    await expect
      .poll(
        async () =>
          (await page.getByRole("heading", { level: 1 }).textContent()) ===
          performer.name,
        { timeout: 8_000 },
      )
      .toBe(true);
    const performerDetail = await read<Schema<"ArtistDetail">>(
      `/api/v1/artists/${performer.id}`,
    );
    verifySpotifyPerformerCredit(
      performerDetail,
      performer,
      byRecording,
      albums,
    );
    liveCheck(
      await page
        .locator("audio[data-hito-player]")
        .evaluate(
          (audio: HTMLAudioElement) =>
            audio.paused && audio.currentTime === 0 && !audio.currentSrc,
        ),
    );
    liveCheck(
      isDeepStrictEqual(
        await read<Schema<"PlaylistDetail">>(
          `/api/v1/playlists/${receipt.playlist_id}`,
        ),
        playlist,
      ),
    );

    phase = 11;
    await Promise.allSettled([...pending]);
    privacyFailed ||= !captured.every((message) =>
      [...privateValues].every(
        (value) =>
          !message.includes(JSON.stringify(value)) &&
          (value.length < 8 || !message.includes(value)),
      ),
    );
    liveCheck(!privacyFailed);
    liveCheck(
      imports === 1 &&
        requests <= 2000 &&
        audioRequests +
          foreignRequests +
          forbiddenWrites +
          downloads +
          extraPages +
          pageErrors ===
          0,
    );
  } catch {
    failed = true;
  } finally {
    try {
      await page.goto("about:blank", { timeout: 5000 });
    } catch {
      failed = true;
      await page.close().catch(() => {
        privacyFailed = true;
      });
    }
    try {
      await plugin?.dispose();
    } catch {
      failed = true;
    }
  }
  failed ||= privacyFailed;
  const evidence = {
    event: failed
      ? "spotify_albums_live_failed"
      : "spotify_albums_live_verified",
    phase,
    phase8_step: phase8Step,
    edition_checks: editionChecks,
    browser_family:
      browserName === "webkit" ? 3 : browserName === "firefox" ? 2 : 1,
    entries: verifiedEntries,
    recordings: verifiedRecordings,
    albums: verifiedAlbums,
    artists: verifiedArtists,
    duplicate_entries: duplicateEntries,
    provider_pages: upstreamPages,
    imports,
    browser_requests: requests,
    public_reads: publicApi.requests,
    public_read_status: publicApi.lastStatus,
    audio_requests: audioRequests,
    foreign_requests: foreignRequests,
    forbidden_writes: forbiddenWrites,
    downloads,
    extra_pages: extraPages,
    page_errors: pageErrors,
    console_privacy_passed: !privacyFailed,
  };
  await info.attach("spotify-albums-live-numerical-evidence", {
    body: Buffer.from(JSON.stringify(evidence)),
    contentType: "application/json",
  });
  console.info(JSON.stringify(evidence));
  if (failed)
    throw new Error(`Live Spotify album verification failed at phase ${phase}`);
});
