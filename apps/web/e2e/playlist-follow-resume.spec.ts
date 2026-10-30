import {
  expect,
  test,
  type Page,
  type Request as BrowserRequest,
  type Route,
} from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import { playlistFollowFixture } from "./fixtures/playlist-follow";
import {
  connectLiveProvider,
  setUpSpotifyYouTubeLiveAccount as setUpDisposableFollowAccount,
} from "./fixtures/spotify-youtube-live";
import { openLibrarySection } from "./fixtures/library-sections";

const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

test.use({ trace: "off", screenshot: "off", video: "off" });

test.describe("isolated provider playlist Resume", () => {
  test.skip(
    process.env.HITO_PLAYLIST_FOLLOW_E2E !== "1",
    "Requires explicit disposable Follow server opt-in",
  );

  test("reviewed updates resume once after a lost acknowledgement and preserve saved content until publication", async ({
    page,
    baseURL,
  }) => {
    const fixture = await playlistFollowFixture();
    const origin = baseURL ? new URL(baseURL).origin : null;
    const syncUrl = origin ? new URL("/api/v1/sync", origin).href : null;
    let phase = 0;
    let failed = false;
    let installedRoute = false;
    let playlist: string | null = null;
    let offers = 0;
    let imports = 0;
    let follows = 0;
    let manualRefreshes = 0;
    let personalWrites = 0;
    let resumePosts = 0;
    let lostDeliveries = 0;
    let committedAcknowledgements = 0;
    let failedResumeRequests = 0;
    let unexpectedResumeFailures = 0;
    let pageErrors = 0;
    let externalRequests = 0;
    let audioRequests = 0;
    let lostRequest: BrowserRequest | null = null;
    let originalBatch: Schema<"SyncBatch"> | null = null;
    let originalAcknowledgement: Schema<"SyncReceipt"> | null = null;
    const resumeBodies: string[] = [];
    const loseAcknowledgement = async (route: Route) => {
      const request = route.request();
      if (!resumeRequest(request, origin, playlist) || lostRequest !== null) {
        await route.continue();
        return;
      }
      lostRequest = request;
      originalBatch = request.postDataJSON();
      let response: Awaited<ReturnType<Route["fetch"]>> | null = null;
      try {
        response = await route.fetch({ maxRetries: 0 });
        expect(response.status()).toBe(200);
        const acknowledgement: Schema<"SyncReceipt"> = await response.json();
        if (!originalBatch)
          throw new Error("The original Resume batch is missing");
        same(acknowledgement, {
          acknowledged: [originalBatch.operations[0].id],
        });
        originalAcknowledgement = acknowledgement;
        committedAcknowledgements++;
      } finally {
        try {
          if (response) await response.dispose();
        } finally {
          lostDeliveries++;
          await route.abort("failed");
        }
      }
    };
    page.on("pageerror", () => pageErrors++);
    page.on("requestfailed", (request) => {
      if (!resumeRequest(request, origin, playlist)) return;
      if (request === lostRequest) failedResumeRequests++;
      else unexpectedResumeFailures++;
    });
    page.on("request", (request) => {
      const url = new URL(request.url());
      if (!["http:", "https:"].includes(url.protocol)) return;
      if (url.origin !== origin) externalRequests++;
      if (request.resourceType() === "media" || /\/audio$/.test(url.pathname))
        audioRequests++;
      if (request.method() !== "POST") return;
      if (/\/plugins\/[^/]+\/playlists\/import$/.test(url.pathname)) imports++;
      if (/\/plugins\/[^/]+\/playlists\/follow$/.test(url.pathname)) follows++;
      if (/\/playlists\/[^/]+\/follow\/refresh$/.test(url.pathname))
        manualRefreshes++;
      if (url.pathname !== "/api/v1/sync") return;
      if (resumeRequest(request, origin, playlist)) {
        resumePosts++;
        const body = request.postData();
        if (body === null)
          throw new Error("The Resume request body is missing");
        resumeBodies.push(body);
      } else personalWrites++;
    });

    try {
      phase = 1;
      if (!origin || !syncUrl)
        throw new Error("The disposable Resume server must be configured");
      await setUpDisposableFollowAccount(page);
      const identity = await read<Schema<"User">>(page, "/api/v1/me");
      const connection = await connectLiveProvider(
        page,
        fixture.address,
        fixture.manifest.name,
        () => offers++,
      );
      expect(
        !connection.credential_set &&
          connection.manifest.capabilities.join(",") === "playlists",
      ).toBe(true);
      const settings = await read<Schema<"PluginSettings">>(
        page,
        "/api/v1/plugins",
      );
      const service = settings.services.find(
        (value) => value.id === connection.service_id,
      );
      if (!service) throw new Error("The original offered service is missing");

      phase = 2;
      const copied = await savePlaylist(
        page,
        connection,
        fixture.reference,
        "copy",
      );
      const copyPath = `/api/v1/playlists/${copied.playlist_id}`;
      const table = page.getByRole("table", {
        name: "Playlist entries",
        exact: true,
      });
      await expect(table.locator("tbody").getByRole("row")).toHaveCount(5);
      await table
        .locator("tbody")
        .getByRole("row")
        .first()
        .getByRole("link", {
          name: `Details for ${fixture.recordings.a.title}`,
          exact: true,
        })
        .click();
      const editor = page.getByRole("region", {
        name: "Your track details",
        exact: true,
      });
      for (const [field, value] of [
        ["title", "My retained Resume title"],
        ["artist", "My retained Resume artist"],
      ] as const) {
        await editor
          .getByRole("textbox", { name: `Track ${field}`, exact: true })
          .fill(value);
        const changed = postResponse(page, "/api/v1/sync");
        await editor
          .getByRole("button", { name: `Save personal ${field}`, exact: true })
          .click();
        expect((await changed).status()).toBe(200);
        await expect(
          editor.getByRole("button", {
            name: `Use source ${field}`,
            exact: true,
          }),
        ).toBeVisible();
      }
      const copyBefore = await read<Schema<"PlaylistDetail">>(page, copyPath);
      const recording = copyBefore.entries[0].recording_id;
      if (!recording || !uuid.test(recording))
        throw new Error("The retained recording identity is missing");
      const metadataPath = `/api/v1/recordings/${recording}/metadata`;
      const metadataBefore = await read<Schema<"RecordingMetadata">>(
        page,
        metadataPath,
      );
      expect(
        metadataBefore.title.personal === "My retained Resume title" &&
          metadataBefore.artist.personal === "My retained Resume artist",
      ).toBe(true);
      expect(
        copyBefore.playlist.kind === "local" && copyBefore.entries.length === 5,
      ).toBe(true);

      phase = 3;
      const followed = await savePlaylist(
        page,
        connection,
        fixture.reference,
        "follow",
      );
      playlist = followed.playlist_id;
      const followPage = `/playlists/${playlist}`;
      const detailPath = `/api/v1/playlists/${playlist}`;
      const statusPath = `${detailPath}/follow`;
      const before = await read<Schema<"PlaylistDetail">>(page, detailPath);
      const ready = await read<Schema<"PlaylistFollow">>(page, statusPath);
      const feedBefore = await read<Schema<"SyncPage">>(
        page,
        "/api/v1/sync?cursor=0",
      );
      const activityBefore = await read<Schema<"Activity">>(
        page,
        "/api/v1/activity",
      );
      expect(
        before.playlist.kind === "followed" &&
          before.entries.length === 5 &&
          before.follow_content_revision === 1,
      ).toBe(true);
      expect(
        ready.status === "ready" &&
          ready.generation === 1 &&
          ready.content_revision === 1 &&
          ready.interval === "daily",
      ).toBe(true);
      expect(ready.next_refresh_at - ready.last_refreshed_at).toBe(86400);
      expect(playlist !== copied.playlist_id && uuid.test(playlist)).toBe(true);
      expect(new Set(before.entries.map((entry) => entry.entry_id)).size).toBe(
        5,
      );
      expect(
        before.entries.every(
          (entry) => uuid.test(entry.entry_id) && !entry.available,
        ),
      ).toBe(true);
      if (!before.origin)
        throw new Error("The saved Follow provenance is missing");
      expect(
        before.origin.installation_id === connection.id &&
          before.origin.installation_revision === connection.revision &&
          before.origin.service_revision === service.revision,
      ).toBe(true);
      expect(feedBefore.has_more).toBe(false);

      phase = 4;
      const proposed = fixture.reviewManifest();
      await page
        .getByRole("link", { name: "Account and settings", exact: true })
        .click();
      await page
        .getByRole("navigation", { name: "Settings sections" })
        .getByRole("link", { name: "Plugins", exact: true })
        .click();
      const card = page.locator(".plugin-connection").filter({
        has: page.getByRole("heading", {
          name: fixture.manifest.name,
          exact: true,
        }),
      });
      const refreshPath = `/api/v1/plugins/${connection.id}/refresh`;
      const checked = postResponse(page, refreshPath);
      await card
        .getByRole("button", { name: "Check connection", exact: true })
        .click();
      const reviewResponse = await checked;
      expect(reviewResponse.status()).toBe(200);
      const review: Schema<"PluginRefresh"> = await reviewResponse.json();
      if (review.kind !== "review_required")
        throw new Error("The changed manifest must require review");
      same(review.manifest, proposed);
      same(
        await read<Schema<"PluginSettings">>(page, "/api/v1/plugins"),
        settings,
      );
      same(await read<Schema<"PlaylistFollow">>(page, statusPath), ready);
      await expect(
        card.getByRole("heading", {
          name: "Review plugin changes",
          exact: true,
        }),
      ).toBeVisible();
      const acceptedResponse = postResponse(page, refreshPath);
      await card
        .getByRole("button", { name: "Accept changes", exact: true })
        .click();
      const accepted = await acceptedResponse;
      expect(accepted.status()).toBe(200);
      const approved: Schema<"PluginRefresh"> = await accepted.json();
      if (approved.kind !== "updated")
        throw new Error("The reviewed connection was not accepted");
      expect(
        approved.installation.id === connection.id &&
          approved.installation.revision === connection.revision + 1,
      ).toBe(true);
      same(approved.installation.manifest, proposed);
      const paused = await read<Schema<"PlaylistFollow">>(page, statusPath);
      expect(
        paused.status === "paused" &&
          paused.generation === 1 &&
          paused.content_revision === 1 &&
          paused.error === "authority_changed",
      ).toBe(true);
      same(await read<Schema<"PlaylistDetail">>(page, detailPath), before);
      same(
        await read<Schema<"SyncPage">>(page, "/api/v1/sync?cursor=0"),
        feedBefore,
      );
      expect(resumePosts).toBe(0);
      await page.goto(followPage);
      const updates = page.getByRole("region", {
        name: "Playlist updates",
        exact: true,
      });
      await expect(updates.getByText("Paused", { exact: true })).toBeVisible();
      await expect(
        updates.getByRole("button", { name: "Refresh now", exact: true }),
      ).toBeDisabled();
      await expect(table.locator("tbody").getByRole("row")).toHaveCount(5);
      await titles(page, before);

      phase = 5;
      const approval = page.getByRole("region", {
        name: "Resume playlist updates",
        exact: true,
      });
      await approval
        .getByRole("button", { name: "Review connection", exact: true })
        .click();
      await expect(
        approval.getByText(proposed.name, { exact: true }),
      ).toBeVisible();
      await expect(
        approval.getByText(service.name, { exact: true }),
      ).toBeVisible();
      await expect(
        approval.getByText(service.base_url, { exact: true }),
      ).toBeVisible();
      await expect(
        approval.getByText("playlists", { exact: true }),
      ).toBeVisible();
      const resume = approval.getByRole("button", {
        name: "Resume updates",
        exact: true,
      });
      await expect(resume).toBeEnabled();
      expect(resumePosts).toBe(0);
      same(await read<Schema<"PlaylistFollow">>(page, statusPath), paused);
      same(await read<Schema<"PlaylistDetail">>(page, detailPath), before);
      same(
        await read<Schema<"SyncPage">>(page, "/api/v1/sync?cursor=0"),
        feedBefore,
      );
      expect(fixture.counts()).toEqual({
        manifest: 5,
        reference: 2,
        canonical: 2,
        items: 2,
        audio: 0,
        sources: 0,
        other: 0,
        invalid: 0,
        credentials: 0,
      });
      const gate = fixture.advanceAndHold();
      await page.route(syncUrl, loseAcknowledgement);
      installedRoute = true;
      await resume.click();
      await expect.poll(() => failedResumeRequests).toBe(1);
      expect([lostDeliveries, committedAcknowledgements, resumePosts]).toEqual([
        1, 1, 1,
      ]);
      const batch: Schema<"SyncBatch"> = JSON.parse(resumeBodies[0]);
      expect(
        batch.operations.length === 1 &&
          uuid.test(batch.operations[0].id) &&
          batch.device_id.length > 0,
      ).toBe(true);
      same(batch.operations[0].action, {
        kind: "playlist_follow_resume",
        playlist_id: playlist,
        expected_generation: paused.generation,
        installation_revision: approved.installation.revision,
        service_revision: service.revision,
      });
      const retry = approval.getByRole("button", {
        name: "Retry resume request",
        exact: true,
      });
      await expect(retry).toBeEnabled();
      await expect.poll(() => gate.entered, { timeout: 10000 }).toBe(true);
      const running = await read<Schema<"PlaylistFollow">>(page, statusPath);
      expect(
        running.status === "running" &&
          running.generation === 2 &&
          running.content_revision === 1 &&
          running.snapshot === ready.snapshot,
      ).toBe(true);
      same(await read<Schema<"PlaylistDetail">>(page, detailPath), before);
      same(await read<Schema<"PlaylistDetail">>(page, copyPath), copyBefore);
      same(
        await read<Schema<"RecordingMetadata">>(page, metadataPath),
        metadataBefore,
      );
      await expect(table.locator("tbody").getByRole("row")).toHaveCount(5);
      await titles(page, before);
      const replayResponse = postResponse(page, "/api/v1/sync");
      await retry.click();
      const replay = await replayResponse;
      expect(replay.status()).toBe(200);
      same(await replay.json(), originalAcknowledgement);
      expect(
        resumeBodies.length === 2 && resumeBodies[0] === resumeBodies[1],
      ).toBe(true);
      await expect(updates.getByText(/^Updating/)).toBeVisible();
      await expect(
        approval.getByRole("button", {
          name: "Retry resume request",
          exact: true,
        }),
      ).toHaveCount(0);
      const resumedFeed = await read<Schema<"SyncPage">>(
        page,
        "/api/v1/sync?cursor=0",
      );
      expect(
        !resumedFeed.has_more &&
          resumedFeed.operations.length === feedBefore.operations.length + 1,
      ).toBe(true);
      same(resumedFeed.operations.slice(0, -1), feedBefore.operations);
      const resumeEvent = resumedFeed.operations.at(-1);
      expect(
        resumeEvent &&
          "operation" in resumeEvent &&
          resumeEvent.device_id === batch.device_id,
      ).toBe(true);
      if (!resumeEvent || !("operation" in resumeEvent))
        throw new Error("The accepted Resume event is missing");
      same(resumeEvent.operation, batch.operations[0]);
      same(await read<Schema<"PlaylistDetail">>(page, detailPath), before);
      await expect(table.locator("tbody").getByRole("row")).toHaveCount(5);

      phase = 6;
      gate.release();
      await expect
        .poll(
          async () => {
            const status = await read<Schema<"PlaylistFollow">>(
              page,
              statusPath,
            );
            return (
              status.status === "ready" &&
              status.generation === 2 &&
              status.content_revision === 2
            );
          },
          { timeout: 10000 },
        )
        .toBe(true);
      await expect(updates.getByText("Ready", { exact: true })).toBeVisible();
      await expect(table.locator("tbody").getByRole("row")).toHaveCount(4);
      const after = await read<Schema<"PlaylistDetail">>(page, detailPath);
      expect(
        after.follow_content_revision === 2 &&
          after.playlist.id === before.playlist.id &&
          after.playlist.kind === "followed",
      ).toBe(true);
      expect(
        after.origin?.snapshot === fixture.updated.detail.snapshot &&
          after.origin.installation_id === connection.id &&
          after.origin.installation_revision ===
            approved.installation.revision &&
          after.origin.service_revision === service.revision,
      ).toBe(true);
      same(
        after.entries.map((entry) => [
          entry.position,
          entry.origin?.occurrence_id,
        ]),
        [
          [0, "duplicate-a"],
          [1, "added-c"],
          [2, "first-a"],
          [3, "withdrawn"],
        ],
      );
      expect(
        after.entries[0].entry_id === before.entries[4].entry_id &&
          after.entries[2].entry_id === before.entries[0].entry_id &&
          after.entries[3].entry_id === before.entries[3].entry_id,
      ).toBe(true);
      expect(
        after.entries[0].recording_id === recording &&
          after.entries[2].recording_id === recording,
      ).toBe(true);
      expect(
        after.entries.every(
          (entry) => uuid.test(entry.entry_id) && !entry.available,
        ),
      ).toBe(true);
      expect(
        !before.entries.some(
          (entry) => entry.entry_id === after.entries[1].entry_id,
        ),
      ).toBe(true);
      expect(
        !after.entries.some(
          (entry) =>
            entry.entry_id === before.entries[1].entry_id ||
            entry.entry_id === before.entries[2].entry_id,
        ),
      ).toBe(true);
      await titles(page, after);
      same(await read<Schema<"PlaylistDetail">>(page, copyPath), copyBefore);
      same(
        await read<Schema<"RecordingMetadata">>(page, metadataPath),
        metadataBefore,
      );
      same(
        await read<Schema<"Activity">>(page, "/api/v1/activity"),
        activityBefore,
      );
      same(await read<Schema<"User">>(page, "/api/v1/me"), identity);
      const feed = await read<Schema<"SyncPage">>(
        page,
        "/api/v1/sync?cursor=0",
      );
      expect(
        !feed.has_more &&
          feed.operations.length === resumedFeed.operations.length + 1,
      ).toBe(true);
      same(feed.operations.slice(0, -1), resumedFeed.operations);
      const publication = feed.operations.at(-1);
      expect(
        publication &&
          "publication" in publication &&
          publication.publication.kind === "playlist_follow_refreshed",
      ).toBe(true);
      const recordings = new Set(
        [...copyBefore.entries, ...after.entries].flatMap((entry) =>
          entry.recording_id ? [entry.recording_id] : [],
        ),
      );
      expect(recordings.size).toBe(3);
      for (const id of recordings)
        expect(
          (
            await read<Schema<"Source">[]>(
              page,
              `/api/v1/recordings/${id}/sources`,
            )
          ).length,
        ).toBe(0);
      expect(fixture.counts()).toEqual({
        manifest: 5,
        reference: 2,
        canonical: 4,
        items: 3,
        audio: 0,
        sources: 0,
        other: 0,
        invalid: 0,
        credentials: 0,
      });
      expect([
        offers,
        imports,
        follows,
        personalWrites,
        resumePosts,
        manualRefreshes,
      ]).toEqual([1, 1, 1, 2, 2, 0]);
      expect([
        lostDeliveries,
        committedAcknowledgements,
        failedResumeRequests,
        unexpectedResumeFailures,
      ]).toEqual([1, 1, 1, 0]);
      expect([pageErrors, externalRequests, audioRequests]).toEqual([0, 0, 0]);
    } catch {
      failed = true;
    } finally {
      if (installedRoute && syncUrl) {
        try {
          await page.unroute(syncUrl, loseAcknowledgement);
        } catch {
          failed = true;
          phase = 7;
        }
      }
      try {
        await fixture.close();
      } catch {
        failed = true;
        phase = 7;
      }
      console.log(
        JSON.stringify({
          phase,
          offers,
          imports,
          follows,
          personal_writes: personalWrites,
          resume_posts: resumePosts,
          manual_refreshes: manualRefreshes,
          lost_deliveries: lostDeliveries,
          committed_acknowledgements: committedAcknowledgements,
          failed_resume_requests: failedResumeRequests,
          unexpected_resume_failures: unexpectedResumeFailures,
          page_errors: pageErrors,
          external_requests: externalRequests,
          audio_requests: audioRequests,
          ...fixture.counts(),
        }),
      );
    }
    if (failed)
      throw new Error(`The isolated Resume workflow failed at phase ${phase}`);
  });
});

function resumeRequest(
  request: BrowserRequest,
  origin: string | null,
  playlist: string | null,
) {
  const url = new URL(request.url());
  if (
    request.method() !== "POST" ||
    url.origin !== origin ||
    url.pathname !== "/api/v1/sync" ||
    playlist === null
  )
    return false;
  const batch: Schema<"SyncBatch"> = request.postDataJSON();
  return (
    batch.operations.length === 1 &&
    batch.operations[0].action.kind === "playlist_follow_resume" &&
    batch.operations[0].action.playlist_id === playlist
  );
}

async function savePlaylist(
  page: Page,
  connection: Schema<"PluginInstallation">,
  reference: string,
  mode: "copy" | "follow",
) {
  await openLibrarySection(page, "Playlists");
  const form = page.getByRole("region", {
    name: "Import a playlist",
    exact: true,
  });
  const label = mode === "copy" ? "Import a copy" : "Follow playlist";
  await form.getByRole("radio", { name: label, exact: true }).check();
  await form
    .getByRole("combobox", { name: "Playlist service" })
    .selectOption(connection.id);
  if (mode === "follow")
    await form
      .getByRole("combobox", { name: "Refresh interval", exact: true })
      .selectOption({ label: "Every day" });
  await form
    .getByRole("textbox", { name: "Playlist link or identifier" })
    .fill(reference);
  const response = postResponse(
    page,
    `/api/v1/plugins/${connection.id}/playlists/${mode === "copy" ? "import" : "follow"}`,
  );
  await form.getByRole("button", { name: label, exact: true }).click();
  const saved = await response;
  expect(saved.status()).toBe(200);
  const receipt:
    | Schema<"PlaylistImportReceipt">
    | Schema<"PlaylistFollowReceipt"> = await saved.json();
  expect(
    uuid.test(receipt.playlist_id) &&
      uuid.test(receipt.request_id) &&
      receipt.request_id === saved.request().postDataJSON().request_id,
  ).toBe(true);
  await expect
    .poll(
      () =>
        new URL(page.url()).pathname === `/playlists/${receipt.playlist_id}`,
    )
    .toBe(true);
  return receipt;
}

function postResponse(page: Page, path: string) {
  return page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname === path &&
      response.request().method() === "POST",
  );
}

async function read<T>(page: Page, path: string): Promise<T> {
  const response = await page.request.get(path);
  if (response.status() !== 200)
    throw new Error(`Owned Resume read returned HTTP ${response.status()}`);
  return response.json();
}

async function titles(page: Page, detail: Schema<"PlaylistDetail">) {
  same(
    await page
      .getByRole("table", { name: "Playlist entries", exact: true })
      .locator("tbody .track-title, tbody .retained-title")
      .allTextContents(),
    detail.entries.map((entry) => entry.title),
  );
}

function same(actual: unknown, expected: unknown) {
  expect(JSON.stringify(actual) === JSON.stringify(expected)).toBe(true);
}
