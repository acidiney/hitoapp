import { isDeepStrictEqual } from "node:util";
import { expect, test } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import { captureConsolePrivacy } from "./fixtures/console-privacy";
import { releaseCreditReader } from "./fixtures/release-credits";
import { playlistMetadataProvider } from "./fixtures/playlist-metadata-provider";
import { metadataDelivery } from "./fixtures/playlist-metadata-delivery";
import {
  metadataState,
  prepareMetadataCopy,
  verifyMetadataRepair,
} from "./fixtures/playlist-metadata-workflow";
import { openLibrarySection } from "./fixtures/library-sections";

test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});
test.skip(
  process.env.HITO_PLAYLIST_METADATA_E2E !== "1",
  "Requires an explicitly disposable playlist-metadata server",
);

test("a lost album lookup acknowledgement retries its original receipt offline without changing the saved copy", async ({
  page,
  context,
  baseURL,
}) => {
  test.setTimeout(120_000);
  page.setDefaultTimeout(10_000);
  page.setDefaultNavigationTimeout(15_000);
  let stage = "fresh disposable server";
  let failed = false;
  let fixture: Awaited<ReturnType<typeof playlistMetadataProvider>> | null =
    null;
  const read = releaseCreditReader(page);
  const pending = new Set<Promise<void>>();
  const captured: string[] = [];
  let captureBytes = 0;
  let privacyFailed = false;
  const password = "owned disposable playlist metadata password";
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
  try {
    expect(baseURL === "http://127.0.0.1:8080").toBe(true);
    const origin = new URL(baseURL!).origin;
    expect(
      (await read<Schema<"SetupStatus">>("/api/v1/setup")).needs_setup,
    ).toBe(true);
    fixture = await playlistMetadataProvider();
    const owned = fixture;
    const delivery = await metadataDelivery(context, page, origin, owned);
    const { evidence } = delivery;
    const { traffic, bodies } = evidence;

    stage = "setup, connect, import and personal edits";
    const saved = await prepareMetadataCopy(page, owned, password, read);
    const refreshPath = delivery.observe(saved.playlist);
    const before = await metadataState(read, saved.playlist, saved.ids);
    expect(
      before.albums.total === 0 &&
        before.metadata.every(
          (metadata) => metadata.selected_observation?.release_id === null,
        ),
    ).toBe(true);
    expect(
      before.copy.entries[0].recording_id === saved.ids[0] &&
        before.copy.entries[0].position === -10 &&
        before.activity.history[0].position_ms === 4_200 &&
        before.activity.history[0].speed === 1.25,
    ).toBe(true);
    expect(
      owned.calls.detail === 2 &&
        owned.calls.items === 1 &&
        owned.calls.manifest === 3,
    ).toBe(true);
    const panel = page.getByRole("region", {
      name: "Playlist metadata",
      exact: true,
    });
    const table = page.getByRole("table", {
      name: "Playlist entries",
      exact: true,
    });
    await expect(
      panel.getByRole("button", { name: "Find missing albums", exact: true }),
    ).toBeEnabled();
    expect(bodies.length).toBe(0);
    owned.enrich();
    stage = "accepted repair with lost browser acknowledgement";
    await panel
      .getByRole("button", { name: "Find missing albums", exact: true })
      .click();
    await expect(panel.getByRole("alert")).toHaveText(
      "Hito could not confirm the album lookup. Retry the same request to recover its result.",
    );
    await expect(
      panel.getByRole("button", { name: "Find missing albums", exact: true }),
    ).toBeDisabled();
    await expect(
      panel.getByRole("button", { name: "Retry album lookup", exact: true }),
    ).toBeEnabled();
    expect(
      !evidence.failed &&
        evidence.receipt !== null &&
        bodies.length === 1 &&
        traffic.lostFailures === 1,
    ).toBe(true);
    const sent = JSON.parse(
      bodies[0],
    ) as Schema<"RefreshImportedPlaylistMetadata">;
    expect(
      /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(
        sent.request_id,
      ) &&
        sent.request_id !== "00000000-0000-0000-0000-000000000000" &&
        sent.installation_revision === saved.connection.revision,
    ).toBe(true);
    expect(
      Object.keys(sent).sort().join(",") === "installation_revision,request_id",
    ).toBe(true);
    const committed = await metadataState(read, saved.playlist, saved.ids);
    const albumId = verifyMetadataRepair(before, committed, saved.ids, owned);
    const callsAfterCommit = { ...owned.calls };
    const settingsBeforeRetry = traffic.settings;
    expect(
      callsAfterCommit.detail === 4 &&
        callsAfterCommit.items === 2 &&
        callsAfterCommit.offline === 0,
    ).toBe(true);
    stage = "explicit same-request retry while provider is offline";
    const [response] = await Promise.all([
      page.waitForResponse(
        (candidate) =>
          new URL(candidate.url()).pathname === refreshPath &&
          candidate.request().method() === "POST",
        { timeout: 15_000 },
      ),
      panel
        .getByRole("button", { name: "Retry album lookup", exact: true })
        .click(),
    ]);
    expect(response.status()).toBe(200);
    const replay = await response.body();
    expect(evidence.receipt !== null && replay.equals(evidence.receipt)).toBe(
      true,
    );
    await expect(panel.getByRole("status")).toHaveText(
      "2 recordings updated. 0 unchanged. 0 unavailable.",
    );
    await expect(
      panel.getByRole("button", { name: "Retry album lookup", exact: true }),
    ).toHaveCount(0);
    await expect(table.locator("tbody tr")).toHaveCount(5);
    await expect(
      table.getByRole("button", { name: owned.verified[1].title, exact: true }),
    ).toHaveCount(2);
    expect(
      bodies.length === 2 &&
        bodies[0] === bodies[1] &&
        traffic.settings === settingsBeforeRetry &&
        isDeepStrictEqual(owned.calls, callsAfterCommit),
    ).toBe(true);
    expect(
      isDeepStrictEqual(
        await metadataState(read, saved.playlist, saved.ids),
        committed,
      ),
    ).toBe(true);

    stage = "album navigation, unknown album artist and stable track order";
    await openLibrarySection(page, "Albums");
    await expect(
      page.getByText("Album artist not provided", { exact: true }),
    ).toBeVisible();
    await page.locator(`a[href="/albums/${albumId}"]`).first().click();
    await expect(
      page.getByRole("heading", { name: owned.album.title, exact: true }),
    ).toBeVisible();
    await expect(
      page.getByText("Album artist not provided", { exact: true }),
    ).toBeVisible();
    const tracks = page
      .getByRole("table", { name: "Tracks", exact: true })
      .locator("tbody tr");
    await expect(tracks).toHaveCount(2);
    expect(
      isDeepStrictEqual(
        await tracks.locator(".artist-column").allTextContents(),
        [saved.personal.artist, owned.recordings[1].artist],
      ),
    ).toBe(true);
    const album = await read<Schema<"ReleaseDetail">>(
      `/api/v1/releases/${albumId}`,
    );
    expect(
      isDeepStrictEqual(
        album.tracks.items.map((track) => [
          track.id,
          track.disc_number,
          track.track_number,
        ]),
        [
          [saved.ids[0], 1, 2],
          [saved.ids[1], 2, 1],
        ],
      ) &&
        album.tracks.items[0].title === saved.personal.title &&
        album.tracks.items[0].liked &&
        album.tracks.items.every((track) => !track.available),
    ).toBe(true);
    const imported = await page.request.post(
      `/api/v1/plugins/${saved.connection.id}/playlists/import`,
      {
        headers: { "x-hito-request": "1", "x-hito-account": saved.user.id },
        data: saved.importRequest,
        timeout: 10_000,
      },
    );
    expect(imported.status()).toBe(200);
    expect(isDeepStrictEqual(await imported.json(), saved.importReceipt)).toBe(
      true,
    );
    expect(
      isDeepStrictEqual(
        await metadataState(read, saved.playlist, saved.ids),
        committed,
      ),
    ).toBe(true);
    for (const path of [
      ...saved.ids.map((id) => `/api/v1/recordings/${id}/download`),
      `/api/v1/playlists/${saved.playlist}/download`,
    ])
      expect((await page.request.get(path, { timeout: 10_000 })).status()).toBe(
        404,
      );
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
      traffic.foreign +
        traffic.audio +
        traffic.download +
        traffic.source +
        traffic.adoption +
        traffic.pageErrors +
        traffic.unexpectedFailures +
        owned.calls.unexpected +
        owned.calls.unsafe_headers +
        owned.calls.offline,
    ).toBe(0);
    expect(
      !evidence.failed &&
        traffic.total <= 400 &&
        traffic.lostFailures === 1 &&
        isDeepStrictEqual(owned.calls, callsAfterCommit),
    ).toBe(true);
  } catch {
    failed = true;
  } finally {
    try {
      await context.unrouteAll({ behavior: "wait" });
      await page.goto("about:blank", { timeout: 5000 });
    } catch {
      failed = true;
    }
    try {
      await fixture?.close();
    } catch {
      failed = true;
    }
    await Promise.allSettled([...pending]);
  }
  if (failed)
    throw new Error(
      `Playlist metadata browser verification failed at ${stage}`,
    );
});
