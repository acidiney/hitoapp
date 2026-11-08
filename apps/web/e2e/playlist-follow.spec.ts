import {
  expect,
  test,
  type Locator,
  type Page,
  type Request as BrowserRequest,
  type Route,
} from "@playwright/test";
import { openPlaylistImport } from "./fixtures/playlist-dialogs";
import { isDeepStrictEqual } from "node:util";
import type { Schema } from "../src/shared/schema";
import { playlistFollowFixture } from "./fixtures/playlist-follow";
import {
  connectLiveProvider,
  setUpSpotifyYouTubeLiveAccount as setUpDisposableFollowAccount,
} from "./fixtures/spotify-youtube-live";
import { openLibrarySection } from "./fixtures/library-sections";
import { enterEditMode } from "./fixtures/playlist-edit";
import { openMetadataEditors } from "./fixtures/track-panels";

const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

test.use({ trace: "off", screenshot: "off", video: "off" });

test.describe("isolated provider playlist Follow", () => {
  test.skip(
    process.env.HITO_PLAYLIST_FOLLOW_E2E !== "1",
    "Requires explicit disposable Follow server opt-in",
  );

  test("a daily Follow refreshes witnessed occurrences while its editable copy and private edits stay intact", async ({
    page,
    baseURL,
  }) => {
    let phase = 0;
    let failed = false;
    let pageErrors = 0;
    let externalRequests = 0;
    let audioRequests = 0;
    let imports = 0;
    let follows = 0;
    let refreshes = 0;
    let outageCheckpoint = 0;
    let distinctRefreshRequests = false;
    let distinctRefreshJobs = false;
    let privateEdits = 0;
    let offers = 0;
    let ownedFollowPath: string | null = null;
    let reloadRouteInstalled = false;
    let partialReloadAttempts = 0;
    let injectedReloadFailures = 0;
    let receivedReloadFailures = 0;
    let successfulReloads = 0;
    let focusStatusResponses = 0;
    let focusStatusUnchanged = 0;
    let focusReceivedEvents = 0;
    let focusHasFocus = 0;
    let focusVisible = 0;
    let unexpectedReloadFailures = 0;
    let unexpectedReloadResponses = 0;
    let injectedRequest: BrowserRequest | null = null;
    const origin = baseURL ? new URL(baseURL).origin : null;
    const reloadUrl = (url: URL) =>
      url.origin === origin && url.pathname === ownedFollowPath;
    const reloadRoute = async (route: Route) => {
      const request = route.request();
      if (!partialDetailRequest(request, origin, ownedFollowPath)) {
        await route.continue();
        return;
      }
      partialReloadAttempts++;
      if (injectedRequest === null) {
        injectedRequest = request;
        injectedReloadFailures++;
        await route.abort("failed");
        return;
      }
      await route.continue();
    };
    const fixture = await playlistFollowFixture();
    let outage: ReturnType<typeof fixture.failNextItems> | null = null;
    page.on("pageerror", () => pageErrors++);
    page.on("requestfailed", (request) => {
      if (!partialDetailRequest(request, origin, ownedFollowPath)) return;
      if (request === injectedRequest) receivedReloadFailures++;
      else unexpectedReloadFailures++;
    });
    page.on("response", (response) => {
      if (!partialDetailRequest(response.request(), origin, ownedFollowPath))
        return;
      if (
        response.status() === 200 &&
        response.headers()["x-inertia"] === "true"
      )
        successfulReloads++;
      else unexpectedReloadResponses++;
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
        refreshes++;
      if (url.pathname === "/api/v1/sync") privateEdits++;
    });
    try {
      phase = 1;
      if (!origin)
        throw new Error("The disposable Follow server must be configured");
      await setUpDisposableFollowAccount(page);
      const connection = await connectLiveProvider(
        page,
        fixture.address,
        fixture.manifest.name,
        () => offers++,
      );
      expect(connection.manifest.capabilities.join(",") === "playlists").toBe(
        true,
      );
      expect(!connection.credential_set).toBe(true);

      phase = 2;
      await openPlaylists(page);
      await openPlaylistImport(page);
      const form = page.getByRole("region", {
        name: "Import a playlist",
        exact: true,
      });
      await form
        .getByRole("radio", { name: "Import a copy", exact: true })
        .check();
      await form
        .getByRole("textbox", { name: "Playlist link or identifier" })
        .fill(fixture.reference);
      const copyResponse = postResponse(
        page,
        `/api/v1/plugins/${connection.id}/playlists/import`,
      );
      await form
        .getByRole("button", { name: "Import a copy", exact: true })
        .click();
      const copiedResponse = await copyResponse;
      expect(copiedResponse.status()).toBe(200);
      const copyReceipt: Schema<"PlaylistImportReceipt"> =
        await copiedResponse.json();
      const copyRequest: Schema<"ImportProviderPlaylist"> = copiedResponse
        .request()
        .postDataJSON();
      expect(
        uuid.test(copyReceipt.playlist_id) &&
          copyReceipt.request_id === copyRequest.request_id,
      ).toBe(true);
      expect(copyRequest.playlist_reference === fixture.reference).toBe(true);
      await expect
        .poll(
          () =>
            new URL(page.url()).pathname ===
            `/playlists/${copyReceipt.playlist_id}`,
        )
        .toBe(true);
      const copyPath = `/api/v1/playlists/${copyReceipt.playlist_id}`;
      const copied = await read<Schema<"PlaylistDetail">>(page, copyPath);
      expect(
        copied.playlist.kind === "local" && copied.entries.length === 5,
      ).toBe(true);
      const table = page.getByRole("table", {
        name: "Playlist entries",
        exact: true,
      });
      await expect(table.locator("tbody").getByRole("row")).toHaveCount(5);
      await enterEditMode(page);
      await expect(
        table.getByRole("button", { name: /^Remove .* from playlist$/ }),
      ).toHaveCount(5);
      const recording = copied.entries[0].recording_id;
      if (!recording || !uuid.test(recording))
        throw new Error("The owned recording identity is missing");

      phase = 3;
      await table
        .locator("tbody")
        .getByRole("row")
        .first()
        .getByRole("link", { name: fixture.recordings.a.title, exact: true })
        .click();
      await openMetadataEditors(page);
      const editor = page.getByRole("region", {
        name: "Details",
        exact: true,
      });
      for (const [field, value] of [
        ["title", "My retained Follow title"],
        ["artist", "My retained Follow artist"],
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
      const metadataPath = `/api/v1/recordings/${recording}/metadata`;
      const metadataBefore = await read<Schema<"RecordingMetadata">>(
        page,
        metadataPath,
      );
      expect(metadataBefore.title.personal === "My retained Follow title").toBe(
        true,
      );
      expect(
        metadataBefore.artist.personal === "My retained Follow artist",
      ).toBe(true);
      const copyBefore = await read<Schema<"PlaylistDetail">>(page, copyPath);

      phase = 4;
      await openPlaylists(page);
      await form
        .getByRole("radio", { name: "Follow playlist", exact: true })
        .check();
      await form
        .getByRole("combobox", { name: "Refresh interval", exact: true })
        .selectOption({ label: "Every day" });
      await form
        .getByRole("textbox", { name: "Playlist link or identifier" })
        .fill(fixture.reference);
      const followedResponse = postResponse(
        page,
        `/api/v1/plugins/${connection.id}/playlists/follow`,
      );
      await form
        .getByRole("button", { name: "Follow playlist", exact: true })
        .click();
      const created = await followedResponse;
      expect(created.status()).toBe(200);
      const followReceipt: Schema<"PlaylistFollowReceipt"> =
        await created.json();
      const followRequest: Schema<"FollowProviderPlaylist"> = created
        .request()
        .postDataJSON();
      expect(
        uuid.test(followReceipt.playlist_id) &&
          followReceipt.request_id === followRequest.request_id,
      ).toBe(true);
      expect(
        followRequest.interval === "daily" &&
          followRequest.playlist_reference === fixture.reference,
      ).toBe(true);
      expect(followReceipt.playlist_id !== copyReceipt.playlist_id).toBe(true);
      await expect
        .poll(
          () =>
            new URL(page.url()).pathname ===
            `/playlists/${followReceipt.playlist_id}`,
        )
        .toBe(true);
      const detailPath = `/api/v1/playlists/${followReceipt.playlist_id}`;
      ownedFollowPath = `/playlists/${followReceipt.playlist_id}`;
      const statusPath = `${detailPath}/follow`;
      const before = await read<Schema<"PlaylistDetail">>(page, detailPath);
      expect(before.playlist.kind === "followed").toBe(true);
      expect(before.entries.length).toBe(5);
      expect(
        before.entries.every(
          (entry) => uuid.test(entry.entry_id) && !entry.available,
        ),
      ).toBe(true);
      expect(new Set(before.entries.map((entry) => entry.entry_id)).size).toBe(
        5,
      );
      expect(
        before.entries[0].recording_id === recording &&
          before.entries[4].recording_id === recording,
      ).toBe(true);
      expect(
        before.entries[2].origin?.kind === "unknown" &&
          before.entries[2].recording_id === null,
      ).toBe(true);
      expect(
        before.entries[3].origin?.kind === "unavailable" &&
          before.entries[3].recording_id === null,
      ).toBe(true);
      samePrivateState(
        before.entries.map((entry) => entry.origin?.occurrence_id),
        ["first-a", "removed-b", "unknown", "withdrawn", "duplicate-a"],
      );
      const updates = page.getByRole("region", {
        name: "Playlist updates",
        exact: true,
      });
      await expect(updates.getByText("Ready", { exact: true })).toBeVisible();
      await expect(
        updates.getByText("Every day", { exact: true }),
      ).toBeVisible();
      await expect(page.getByText(/Read-only playlist/)).toBeVisible();
      await expect(page.getByText(/Editable copy/)).toHaveCount(0);
      await expect(
        table.getByRole("button", { name: /^Remove .* from playlist$/ }),
      ).toHaveCount(0);
      await expect(
        page.getByRole("button", { name: "Play playlist", exact: true }),
      ).toBeDisabled();
      const ready = await read<Schema<"PlaylistFollow">>(page, statusPath);
      expect(ready.status === "ready" && ready.interval === "daily").toBe(true);
      expect(ready.next_refresh_at - ready.last_refreshed_at).toBe(86400);
      const feedBefore = await read<Schema<"SyncPage">>(
        page,
        "/api/v1/sync?cursor=0",
      );
      expect(feedBefore.has_more).toBe(false);

      phase = 5;
      const refresh = updates.getByRole("button", {
        name: "Refresh now",
        exact: true,
      });
      outageCheckpoint = 1;
      outage = fixture.failNextItems(3);
      const [failedDetails, failedResponse] = await Promise.all([
        page.waitForResponse(
          (response) =>
            partialDetailRequest(response.request(), origin, ownedFollowPath),
          { timeout: 15000 },
        ),
        (async () => {
          const response = postResponse(page, `${statusPath}/refresh`);
          await refresh.click();
          const queued = await response;
          expect(queued.status()).toBe(202);
          await expect(
            updates.getByText("Update failed", { exact: true }),
          ).toBeVisible({ timeout: 15000 });
          return queued;
        })(),
      ]);
      outageCheckpoint = 2;
      const failedRequest: Schema<"RefreshFollowedPlaylist"> = failedResponse
        .request()
        .postDataJSON();
      const failedReceipt: Schema<"PlaylistFollowRefresh"> =
        await failedResponse.json();
      expect(
        uuid.test(failedReceipt.job_id) &&
          failedReceipt.request_id === failedRequest.request_id &&
          failedReceipt.playlist_id === before.playlist.id,
      ).toBe(true);
      outageCheckpoint = 3;
      expect(failedDetails.status()).toBe(200);
      expect(failedDetails.headers()["x-inertia"]).toBe("true");
      expect(await failedDetails.finished()).toBe(null);
      const retained = await failedDetails.json();
      expect(retained.component === "PlaylistPage").toBe(true);
      outageCheckpoint = 4;
      samePrivateState(retained.props.detail, before);
      outageCheckpoint = 5;
      expect([outage.failures, outage.remaining, refreshes]).toEqual([3, 0, 1]);
      const unavailable = await read<Schema<"PlaylistFollow">>(
        page,
        statusPath,
      );
      expect(
        unavailable.status === "failed" &&
          unavailable.error === "refresh_failed" &&
          unavailable.snapshot === ready.snapshot &&
          unavailable.content_revision === ready.content_revision &&
          unavailable.generation === ready.generation &&
          unavailable.last_refreshed_at === ready.last_refreshed_at,
      ).toBe(true);
      outageCheckpoint = 6;
      await expect(
        updates.getByText(
          "The last update failed. Your saved entries remain available.",
          { exact: true },
        ),
      ).toBeVisible();
      await expect(refresh).toBeEnabled();
      await expect(table.locator("tbody").getByRole("row")).toHaveCount(5);
      await retainedTitles(
        table,
        before.entries.map((entry) => entry.title),
      );
      outageCheckpoint = 7;
      samePrivateState(
        await read<Schema<"PlaylistDetail">>(page, detailPath),
        before,
      );
      samePrivateState(
        await read<Schema<"PlaylistDetail">>(page, copyPath),
        copyBefore,
      );
      samePrivateState(
        await read<Schema<"RecordingMetadata">>(page, metadataPath),
        metadataBefore,
      );
      samePrivateState(
        await read<Schema<"SyncPage">>(page, "/api/v1/sync?cursor=0"),
        feedBefore,
      );
      outageCheckpoint = 8;
      expect(fixture.counts()).toEqual({
        manifest: 3,
        reference: 2,
        canonical: 5,
        items: 5,
        audio: 0,
        sources: 0,
        other: 0,
        invalid: 0,
        credentials: 0,
      });

      outageCheckpoint = 9;
      const gate = fixture.advanceAndHold();
      const refreshResponse = postResponse(page, `${statusPath}/refresh`);
      await refresh.click();
      const queued = await refreshResponse;
      expect(queued.status()).toBe(202);
      const request: Schema<"RefreshFollowedPlaylist"> = queued
        .request()
        .postDataJSON();
      const receipt: Schema<"PlaylistFollowRefresh"> = await queued.json();
      expect(
        uuid.test(receipt.job_id) &&
          receipt.request_id === request.request_id &&
          receipt.playlist_id === before.playlist.id,
      ).toBe(true);
      distinctRefreshRequests = request.request_id !== failedRequest.request_id;
      distinctRefreshJobs = receipt.job_id !== failedReceipt.job_id;
      expect(distinctRefreshRequests && distinctRefreshJobs).toBe(true);
      outageCheckpoint = 10;
      await expect.poll(() => gate.entered, { timeout: 10000 }).toBe(true);
      await expect(updates.getByText(/^Updating/)).toBeVisible();
      await expect(refresh).toBeDisabled();
      await expect(table.locator("tbody").getByRole("row")).toHaveCount(5);
      await retainedTitles(
        table,
        before.entries.map((entry) => entry.title),
      );
      samePrivateState(
        await read<Schema<"PlaylistDetail">>(page, detailPath),
        before,
      );
      const running = await read<Schema<"PlaylistFollow">>(page, statusPath);
      expect(
        running.status === "running" && running.snapshot === ready.snapshot,
      ).toBe(true);
      samePrivateState(
        await read<Schema<"PlaylistDetail">>(page, copyPath),
        copyBefore,
      );

      phase = 6;
      await page.route(reloadUrl, reloadRoute);
      reloadRouteInstalled = true;
      gate.release();
      await expect
        .poll(
          async () =>
            (await read<Schema<"PlaylistFollow">>(page, statusPath)).status ===
            "ready",
          { timeout: 10000 },
        )
        .toBe(true);
      const completedStatus = await read<Schema<"PlaylistFollow">>(
        page,
        statusPath,
      );
      await expect.poll(() => receivedReloadFailures).toBe(1);
      expect([partialReloadAttempts, injectedReloadFailures]).toEqual([1, 1]);
      await expect(updates.getByText("Ready", { exact: true })).toBeVisible();
      await expect(table.locator("tbody").getByRole("row")).toHaveCount(5);
      await retainedTitles(
        table,
        before.entries.map((entry) => entry.title),
      );
      const [, delivered] = await Promise.all([
        page
          .waitForResponse(
            (response) => {
              const url = new URL(response.url());
              const selected =
                url.origin === origin &&
                url.pathname === statusPath &&
                response.request().method() === "GET";
              if (selected) focusStatusResponses++;
              return selected;
            },
            { timeout: 10000 },
          )
          .then(async (checked) => {
            expect(checked.status()).toBe(200);
            samePrivateState(await checked.json(), completedStatus);
            focusStatusUnchanged++;
          }),
        page.waitForResponse(
          (response) =>
            partialDetailRequest(response.request(), origin, ownedFollowPath),
          { timeout: 10000 },
        ),
        page
          .evaluate(() => {
            let events = 0;
            const observed = () => events++;
            window.addEventListener("focus", observed);
            try {
              window.dispatchEvent(new Event("focus"));
              return {
                events,
                hasFocus: Number(document.hasFocus()),
                visible: Number(document.visibilityState === "visible"),
              };
            } finally {
              window.removeEventListener("focus", observed);
            }
          })
          .then((focus) => {
            focusReceivedEvents = focus.events;
            focusHasFocus = focus.hasFocus;
            focusVisible = focus.visible;
          }),
      ]);
      expect(focusReceivedEvents).toBe(1);
      expect(focusStatusUnchanged).toBe(1);
      expect(delivered.status()).toBe(200);
      expect(delivered.headers()["x-inertia"]).toBe("true");
      expect(await delivered.finished()).toBe(null);
      const reloaded = await delivered.json();
      expect(
        reloaded.component === "PlaylistPage" &&
          reloaded.props.detail.playlist.id === followReceipt.playlist_id &&
          reloaded.props.detail.entries.length === 4,
      ).toBe(true);
      await expect(table.locator("tbody").getByRole("row")).toHaveCount(4);
      await expect(refresh).toBeEnabled();
      const after = await read<Schema<"PlaylistDetail">>(page, detailPath);
      expect(
        after.playlist.id === before.playlist.id &&
          after.playlist.kind === "followed",
      ).toBe(true);
      expect(after.entries.every((entry) => uuid.test(entry.entry_id))).toBe(
        true,
      );
      samePrivateState(
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
        after.origin?.provider === fixture.manifest.id &&
          after.origin.installation_id === connection.id,
      ).toBe(true);
      expect(
        after.origin?.provider_playlist_id ===
          before.origin?.provider_playlist_id &&
          after.origin?.snapshot === fixture.updated.detail.snapshot,
      ).toBe(true);
      expect(after.entries[0].entry_id === before.entries[4].entry_id).toBe(
        true,
      );
      expect(after.entries[2].entry_id === before.entries[0].entry_id).toBe(
        true,
      );
      expect(after.entries[3].entry_id === before.entries[3].entry_id).toBe(
        true,
      );
      expect(
        after.entries[0].recording_id === recording &&
          after.entries[2].recording_id === recording,
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
      expect(after.entries.every((entry) => !entry.available)).toBe(true);
      await retainedTitles(
        table,
        after.entries.map((entry) => entry.title),
      );
      samePrivateState(
        await read<Schema<"PlaylistDetail">>(page, copyPath),
        copyBefore,
      );
      samePrivateState(
        await read<Schema<"RecordingMetadata">>(page, metadataPath),
        metadataBefore,
      );
      const feed = await read<Schema<"SyncPage">>(
        page,
        "/api/v1/sync?cursor=0",
      );
      expect(
        !feed.has_more &&
          feed.operations.length === feedBefore.operations.length + 1,
      ).toBe(true);
      samePrivateState(feed.operations.slice(0, -1), feedBefore.operations);
      const event = feed.operations.at(-1);
      expect(
        event &&
          "publication" in event &&
          event.publication.kind === "playlist_follow_refreshed",
      ).toBe(true);
      const recordingIds = new Set(
        [...copyBefore.entries, ...after.entries].flatMap((entry) =>
          entry.recording_id ? [entry.recording_id] : [],
        ),
      );
      expect(recordingIds.size).toBe(3);
      for (const id of recordingIds) {
        expect(uuid.test(id)).toBe(true);
        expect(
          (
            await read<Schema<"Source">[]>(
              page,
              `/api/v1/recordings/${id}/sources`,
            )
          ).length,
        ).toBe(0);
      }
      expect(fixture.counts()).toEqual({
        manifest: 3,
        reference: 2,
        canonical: 7,
        items: 6,
        audio: 0,
        sources: 0,
        other: 0,
        invalid: 0,
        credentials: 0,
      });
      expect([offers, imports, follows, refreshes, privateEdits]).toEqual([
        1, 1, 1, 2, 2,
      ]);
      expect([pageErrors, externalRequests, audioRequests]).toEqual([0, 0, 0]);
      expect([injectedReloadFailures, receivedReloadFailures]).toEqual([1, 1]);
      expect(partialReloadAttempts).toBeGreaterThanOrEqual(2);
      expect(successfulReloads).toBeGreaterThanOrEqual(1);
      expect(focusStatusResponses).toBeGreaterThanOrEqual(1);
      expect([unexpectedReloadFailures, unexpectedReloadResponses]).toEqual([
        0, 0,
      ]);
      console.log(
        JSON.stringify({
          phase,
          copied_entries: copyBefore.entries.length,
          followed_entries_before: before.entries.length,
          followed_entries_after: after.entries.length,
          stable_witnessed_entries: 3,
          private_edits: privateEdits,
          refreshes,
          provider_item_failures: outage.failures,
          outage_checkpoint: outageCheckpoint,
          distinct_refresh_requests: distinctRefreshRequests,
          distinct_refresh_jobs: distinctRefreshJobs,
          partial_reload_attempts: partialReloadAttempts,
          injected_reload_failures: injectedReloadFailures,
          received_reload_failures: receivedReloadFailures,
          successful_detail_reloads: successfulReloads,
          focus_status_responses: focusStatusResponses,
          focus_status_unchanged: focusStatusUnchanged,
          focus_received_events: focusReceivedEvents,
          focus_has_focus: focusHasFocus,
          focus_visible: focusVisible,
          ...fixture.counts(),
        }),
      );
    } catch {
      failed = true;
      console.log(
        JSON.stringify({
          phase,
          provider_item_failures: outage?.failures ?? 0,
          outage_checkpoint: outageCheckpoint,
          distinct_refresh_requests: distinctRefreshRequests,
          distinct_refresh_jobs: distinctRefreshJobs,
          partial_reload_attempts: partialReloadAttempts,
          injected_reload_failures: injectedReloadFailures,
          received_reload_failures: receivedReloadFailures,
          successful_detail_reloads: successfulReloads,
          focus_status_responses: focusStatusResponses,
          focus_status_unchanged: focusStatusUnchanged,
          focus_received_events: focusReceivedEvents,
          focus_has_focus: focusHasFocus,
          focus_visible: focusVisible,
          unexpected_reload_failures: unexpectedReloadFailures,
          unexpected_reload_responses: unexpectedReloadResponses,
          page_errors: pageErrors,
        }),
      );
    } finally {
      if (reloadRouteInstalled) {
        try {
          await page.unroute(reloadUrl, reloadRoute);
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
    }
    if (failed)
      throw new Error(`The isolated Follow workflow failed at phase ${phase}`);
  });
});

function partialDetailRequest(
  request: BrowserRequest,
  origin: string | null,
  path: string | null,
) {
  const url = new URL(request.url());
  if (
    request.method() !== "GET" ||
    url.origin !== origin ||
    url.pathname !== path
  )
    return false;
  const headers = request.headers();
  return (
    headers["x-inertia"] === "true" &&
    headers["x-inertia-partial-component"] === "PlaylistPage" &&
    headers["x-inertia-partial-data"]
      ?.split(",")
      .map((field) => field.trim())
      .sort()
      .join(",") === "detail,playlists"
  );
}

async function openPlaylists(page: Page) {
  await openLibrarySection(page, "Playlists");
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
    throw new Error(`Owned Follow read returned HTTP ${response.status()}`);
  return response.json();
}

async function retainedTitles(table: Locator, titles: string[]) {
  const actual = await table
    .locator("tbody .track-title, tbody .retained-title")
    .allTextContents();
  samePrivateState(actual, titles);
}

function samePrivateState(actual: unknown, expected: unknown) {
  expect(isDeepStrictEqual(actual, expected)).toBe(true);
}
