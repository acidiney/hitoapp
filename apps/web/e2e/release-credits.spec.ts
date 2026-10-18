import { expect, test } from "@playwright/test";
import type { components } from "../src/generated";
import { captureConsolePrivacy } from "./fixtures/console-privacy";
import {
  creditAction,
  creditPost,
  releaseCreditReader,
  releaseCreditsFixture,
} from "./fixtures/release-credits";

type Schema = components["schemas"];
test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});
test.skip(
  process.env.HITO_RELEASE_CREDITS_E2E !== "1",
  "Requires an explicitly disposable release-credit server",
);

test("source-free imported albums preserve unknown credits through personal edit and reset", async ({
  page,
  context,
  baseURL,
}) => {
  test.setTimeout(120_000);
  page.setDefaultTimeout(10_000);
  page.setDefaultNavigationTimeout(15_000);
  let stage = "fresh server";
  let failed = false;
  let fixture: Awaited<ReturnType<typeof releaseCreditsFixture>> | null = null;
  const read = releaseCreditReader(page);
  const pending = new Set<Promise<void>>();
  const captured: string[] = [];
  let captureBytes = 0;
  let privacyFailed = false;
  let audioRequests = 0;
  let downloadWrites = 0;
  let foreignRequests = 0;
  let pageErrors = 0;
  let requests = 0;
  const password = "owned disposable release credit password";
  await captureConsolePrivacy(
    context,
    (text) => {
      captureBytes += Buffer.byteLength(text);
      if (captureBytes <= 524_288) captured.push(text);
      else privacyFailed = true;
    },
    () => {
      privacyFailed = true;
    },
    pending,
  );
  page.on("pageerror", () => pageErrors++);
  try {
    expect(baseURL === "http://127.0.0.1:8080").toBe(true);
    const origin = new URL(baseURL!).origin;
    expect(
      (await read<Schema["SetupStatus"]>("/api/v1/setup")).needs_setup,
    ).toBe(true);
    fixture = await releaseCreditsFixture();
    const owned = fixture;
    await context.route("**/*", async (route) => {
      const request = route.request();
      const target = new URL(request.url());
      requests++;
      if (target.origin !== origin) foreignRequests++;
      const audio = /\/sources\/[^/]+\/audio$/.test(target.pathname);
      if (audio) audioRequests++;
      if (request.method() === "POST" && /\/download$/.test(target.pathname))
        downloadWrites++;
      if (target.origin !== origin || audio || requests > 400)
        await route.abort();
      else await route.continue();
    });
    await page.goto("/");
    await page.getByLabel("Username", { exact: true }).fill("admin");
    await page.getByLabel("Password", { exact: true }).fill(password);
    await page
      .getByRole("button", { name: "Create administrator", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    const navigation = page.getByRole("navigation", {
      name: "Main navigation",
    });
    stage = "offer and connect owned provider";
    await navigation
      .getByRole("link", { name: "Settings", exact: true })
      .click();
    await page.getByRole("tab", { name: "Plugins", exact: true }).click();
    await page.getByText("Services for this server", { exact: true }).click();
    await page
      .getByLabel("Service address", { exact: true })
      .fill(owned.address);
    const offered = await creditPost<Schema["PluginService"]>(
      page,
      "/api/v1/plugins/services",
      201,
      () =>
        page
          .getByRole("button", { name: "Offer service", exact: true })
          .click(),
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
    const installed = await creditPost<Schema["PluginInstallation"]>(
      page,
      "/api/v1/plugins",
      201,
      () =>
        page
          .getByRole("button", { name: "Connect service", exact: true })
          .click(),
    );
    const connection = installed.body;
    expect(
      connection.service_id === offered.body.id &&
        connection.credential_set &&
        connection.manifest.capabilities.length === 1 &&
        connection.manifest.capabilities[0] === "playlists",
    ).toBe(true);
    stage = "import metadata without sources";
    await navigation
      .getByRole("link", { name: "Playlists", exact: true })
      .click();
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
        form
          .getByRole("button", { name: "Import a copy", exact: true })
          .click(),
    );
    const sent = imported.sent as Schema["ImportProviderPlaylist"];
    expect(
      imported.body.request_id === sent.request_id &&
        sent.installation_revision === connection.revision &&
        sent.playlist_reference === owned.reference,
    ).toBe(true);
    const playlistId = imported.body.playlist_id;
    await expect(page).toHaveURL(`${origin}/playlists/${playlistId}`);
    const playlist = await read<Schema["PlaylistDetail"]>(
      `/api/v1/playlists/${playlistId}`,
    );
    expect(
      playlist.entries.length === 3 &&
        playlist.entries.every((entry) => !entry.available),
    ).toBe(true);
    const firstId = playlist.entries[1].recording_id;
    const secondId = playlist.entries[0].recording_id;
    if (!firstId || !secondId)
      throw new Error("Imported recordings need identities");
    expect(
      firstId !== secondId &&
        playlist.entries[2].recording_id === secondId &&
        new Set(playlist.entries.map((entry) => entry.entry_id)).size === 3,
    ).toBe(true);
    expect(
      JSON.stringify(
        playlist.entries.map((entry) => [
          entry.position,
          entry.origin?.occurrence_id,
        ]),
      ) ===
        JSON.stringify([
          [0, "second-first"],
          [1, "first-next"],
          [2, "second-again"],
        ]),
    ).toBe(true);
    const releases = await read<Schema["ReleasePage"]>("/api/v1/releases");
    expect(releases.total === 1 && releases.items.length === 1).toBe(true);
    const album = releases.items[0];
    expect(
      album.title === owned.album.title &&
        album.artist === null &&
        album.artist_id === null &&
        album.track_count === 2,
    ).toBe(true);
    const albumPath = `/albums/${album.id}`;
    const detail = await read<Schema["ReleaseDetail"]>(
      `/api/v1/releases/${album.id}`,
    );
    expect(
      JSON.stringify(
        detail.tracks.items.map((track) => [
          track.id,
          track.disc_number,
          track.track_number,
        ]),
      ) ===
        JSON.stringify([
          [firstId, 1, 2],
          [secondId, 2, 1],
        ]),
    ).toBe(true);
    expect(
      detail.tracks.total === 2 &&
        detail.tracks.items.every(
          (track, index) =>
            !track.available &&
            track.release_id === album.id &&
            track.title === owned.recordings[index].title &&
            track.artist === owned.recordings[index].artist &&
            track.duration_ms === owned.recordings[index].duration_ms,
        ),
    ).toBe(true);
    const recordingMetadata: Schema["RecordingMetadata"][] = [];
    for (const id of [firstId, secondId]) {
      expect(
        (await read<Schema["Source"][]>(`/api/v1/recordings/${id}/sources`))
          .length,
      ).toBe(0);
      expect(
        (
          await page.request.get(`/api/v1/recordings/${id}/download`, {
            timeout: 10_000,
          })
        ).status(),
      ).toBe(404);
      recordingMetadata.push(
        await read<Schema["RecordingMetadata"]>(
          `/api/v1/recordings/${id}/metadata`,
        ),
      );
    }
    const initial = await read<Schema["ReleaseMetadata"]>(
      `/api/v1/releases/${album.id}/metadata`,
    );
    expect(
      initial.artist.value === null &&
        initial.artist.inherited === null &&
        initial.artist.personal === null &&
        initial.observations.items.length === 2 &&
        initial.observations.items.every(
          (item) =>
            item.origin === "provider_response" &&
            item.provider === owned.manifest.id &&
            item.plugin_installation_id === connection.id &&
            item.provider_revision === "metadata-one" &&
            item.album_artist === null &&
            item.album === owned.album.title &&
            item.source_id === null &&
            owned.recordings.some(
              (recording) =>
                recording.id === item.provider_recording_id &&
                recording.artist === item.artist,
            ),
        ),
    ).toBe(true);
    stage = "album and performer navigation";
    await navigation.getByRole("link", { name: "Albums", exact: true }).click();
    await expect(
      page.getByText("Album artist not provided", { exact: true }),
    ).toBeVisible();
    await expect(page.locator('a[href^="/artists/"]')).toHaveCount(0);
    await page.locator(`a[href="${albumPath}"]`).first().click();
    await expect(
      page.getByRole("heading", { name: owned.album.title, exact: true }),
    ).toBeVisible();
    await expect(
      page.getByText("Album artist not provided", { exact: true }),
    ).toBeVisible();
    const rows = page
      .getByRole("table", { name: "Tracks", exact: true })
      .locator("tbody tr");
    expect(
      JSON.stringify(await rows.locator(".artist-column").allTextContents()) ===
        JSON.stringify(owned.recordings.map((recording) => recording.artist)),
    ).toBe(true);
    const artists = await read<Schema["ArtistPage"]>("/api/v1/artists");
    expect(
      artists.total === 2 &&
        artists.items.every((artist) =>
          owned.recordings.some(
            (recording) => recording.artist === artist.name,
          ),
        ),
    ).toBe(true);
    await navigation
      .getByRole("link", { name: "Artists", exact: true })
      .click();
    const performer = artists.items[0];
    await page.locator(`a[href="/artists/${performer.id}"]`).click();
    await expect(
      page.getByRole("heading", { name: performer.name, exact: true }),
    ).toBeVisible();
    expect(
      (await read<Schema["ArtistDetail"]>(`/api/v1/artists/${performer.id}`))
        .artist.id === performer.id,
    ).toBe(true);
    await page.locator(`a[href="${albumPath}"]`).first().click();
    await page
      .getByRole("link", { name: "Edit album details", exact: true })
      .click();
    await expect(
      page.getByText("Album details", { exact: true }),
    ).toBeVisible();
    await expect(page.getByLabel("Album artist", { exact: true })).toHaveValue(
      "",
    );
    await expect(
      page.getByText("Not provided by source", { exact: true }),
    ).toBeVisible();
    stage = "personal artist and acknowledged reload";
    const personal = `${owned.prefix} personal credit`;
    await page.getByLabel("Album artist", { exact: true }).fill(personal);
    const savedId = await creditAction(
      page,
      { kind: "release_artist_set", release_id: album.id, artist: personal },
      () =>
        page
          .getByRole("button", { name: "Save personal artist", exact: true })
          .click(),
    );
    await expect(
      page.getByRole("button", { name: "Use source artist", exact: true }),
    ).toBeVisible();
    const saved = await read<Schema["ReleaseMetadata"]>(
      `/api/v1/releases/${album.id}/metadata`,
    );
    expect(
      saved.artist.value === personal &&
        saved.artist.inherited === null &&
        saved.artist.personal === personal &&
        saved.artist.operation_id === savedId &&
        JSON.stringify(saved.observations) ===
          JSON.stringify(initial.observations),
    ).toBe(true);
    await page
      .getByRole("link", { name: "Back to album", exact: true })
      .click();
    const personalized = await read<Schema["ReleaseDetail"]>(
      `/api/v1/releases/${album.id}`,
    );
    expect(
      personalized.release.id === album.id &&
        personalized.release.artist === personal &&
        personalized.release.artist_id !== null &&
        !artists.items.some(
          (artist) => artist.id === personalized.release.artist_id,
        ) &&
        JSON.stringify(personalized.tracks) === JSON.stringify(detail.tracks),
    ).toBe(true);
    await expect(
      page.locator(`a[href="/artists/${personalized.release.artist_id}"]`),
    ).toHaveText(personal);
    await page
      .getByRole("link", { name: "Edit album details", exact: true })
      .click();
    stage = "reset to real unknown credit";
    const resetId = await creditAction(
      page,
      { kind: "release_artist_inherit", release_id: album.id },
      () =>
        page
          .getByRole("button", { name: "Use source artist", exact: true })
          .click(),
    );
    await expect(page.getByLabel("Album artist", { exact: true })).toHaveValue(
      "",
    );
    await page.reload();
    await expect(page.getByLabel("Album artist", { exact: true })).toHaveValue(
      "",
    );
    const reset = await read<Schema["ReleaseMetadata"]>(
      `/api/v1/releases/${album.id}/metadata`,
    );
    expect(
      reset.artist.value === null &&
        reset.artist.inherited === null &&
        reset.artist.personal === null &&
        reset.artist.operation_id === resetId &&
        JSON.stringify(reset.observations) ===
          JSON.stringify(initial.observations),
    ).toBe(true);
    await page
      .getByRole("link", { name: "Back to album", exact: true })
      .click();
    await expect(
      page.getByText("Album artist not provided", { exact: true }),
    ).toBeVisible();
    await expect(page.locator('a[href^="/artists/"]')).toHaveCount(0);
    const final = await read<Schema["ReleaseDetail"]>(
      `/api/v1/releases/${album.id}`,
    );
    expect(JSON.stringify(final) === JSON.stringify(detail)).toBe(true);
    expect(
      JSON.stringify(
        await read<Schema["PlaylistDetail"]>(`/api/v1/playlists/${playlistId}`),
      ) === JSON.stringify(playlist),
    ).toBe(true);
    for (const [index, id] of [firstId, secondId].entries()) {
      expect(
        JSON.stringify(
          await read<Schema["RecordingMetadata"]>(
            `/api/v1/recordings/${id}/metadata`,
          ),
        ) === JSON.stringify(recordingMetadata[index]),
      ).toBe(true);
      expect(
        (await read<Schema["Source"][]>(`/api/v1/recordings/${id}/sources`))
          .length,
      ).toBe(0);
      expect(
        (
          await page.request.get(`/api/v1/recordings/${id}/download`, {
            timeout: 10_000,
          })
        ).status(),
      ).toBe(404);
    }
    expect(
      (
        await page.request.get(`/api/v1/playlists/${playlistId}/download`, {
          timeout: 10_000,
        })
      ).status(),
    ).toBe(404);
    expect(
      await page
        .locator("audio[data-hito-player]")
        .evaluate(
          (audio: HTMLAudioElement) =>
            audio.paused && audio.currentTime === 0 && !audio.currentSrc,
        ),
    ).toBe(true);
    await Promise.allSettled([...pending]);
    expect(
      !privacyFailed &&
        captured.every((text) =>
          [
            owned.prefix,
            owned.token,
            owned.reference,
            owned.address,
            password,
          ].every((value) => !text.includes(value)),
        ),
    ).toBe(true);
    expect(
      audioRequests +
        downloadWrites +
        foreignRequests +
        pageErrors +
        owned.calls.unexpected +
        owned.calls.unsafe_headers,
    ).toBe(0);
    expect(
      owned.calls.detail === 1 &&
        owned.calls.items === 1 &&
        owned.calls.manifest >= 2 &&
        requests <= 400,
    ).toBe(true);
  } catch {
    failed = true;
  } finally {
    try {
      await page.goto("about:blank", { timeout: 5000 });
    } catch {
      failed = true;
    }
    try {
      await fixture?.close();
    } catch {
      failed = true;
    }
  }
  if (failed)
    throw new Error(`Release credit browser verification failed at ${stage}`);
});
