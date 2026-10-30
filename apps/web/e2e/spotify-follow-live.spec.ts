import { isDeepStrictEqual } from "node:util";
import {
  expect,
  test,
  type APIRequestContext,
  type Page,
} from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import {
  connectLiveProvider,
  setUpSpotifyYouTubeLiveAccount as setUpDisposableAccount,
} from "./fixtures/spotify-youtube-live";
import {
  SpotifyFollowLiveFailure,
  spotifyFollowLiveConfiguration,
  verifySpotifyFollowSnapshot,
} from "./fixtures/spotify-follow-live";
import { openLibrarySection } from "./fixtures/library-sections";

const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

test.use({ trace: "off", screenshot: "off", video: "off" });

async function read<T>(page: Page, path: string): Promise<T> {
  const response = await page.request.get(path);
  if (response.status() !== 200) throw new SpotifyFollowLiveFailure(2);
  return response.json();
}

async function visibleFollow(page: Page, detail: Schema<"PlaylistDetail">) {
  expect(
    (await page.getByRole("heading", { level: 1 }).textContent()) ===
      detail.playlist.name,
  ).toBe(true);
  await expect(page.getByText(/Read-only playlist\./)).toBeVisible();
  await expect(
    page.getByRole("button", { name: /^Remove .* from playlist$/ }),
  ).toHaveCount(0);
  if (detail.entries.length === 0) {
    await expect(
      page.getByText("The provider's playlist is empty", { exact: true }),
    ).toBeVisible();
    return;
  }
  const table = page.getByRole("table", {
    name: "Playlist entries",
    exact: true,
  });
  await expect(table.locator("tbody").getByRole("row")).toHaveCount(
    detail.entries.length,
  );
  const buttons = await table.getByRole("button", { name: /^Play / }).all();
  expect(
    (await Promise.all(buttons.map((button) => button.isDisabled()))).every(
      Boolean,
    ),
  ).toBe(true);
  await expect(
    page.getByRole("button", { name: "Play playlist", exact: true }),
  ).toBeDisabled();
}

async function createFollow(
  page: Page,
  connection: Schema<"PluginInstallation">,
  reference: string,
  interval: Schema<"PlaylistFollowInterval">,
) {
  await openLibrarySection(page, "Playlists");
  const form = page.getByRole("region", {
    name: "Import a playlist",
    exact: true,
  });
  await form
    .getByRole("radio", { name: "Follow playlist", exact: true })
    .check();

  const refreshInterval = form.getByRole("combobox", {
    name: "Refresh interval",
    exact: true,
  });
  await expect(refreshInterval).toHaveValue("hourly");
  await refreshInterval.selectOption(interval);
  await form
    .getByRole("textbox", { name: "Playlist link or identifier", exact: true })
    .fill(reference);
  const followedResponse = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname ===
        `/api/v1/plugins/${connection.id}/playlists/follow` &&
      response.request().method() === "POST",
    { timeout: 60_000 },
  );
  await form
    .getByRole("button", { name: "Follow playlist", exact: true })
    .click();
  const followed = await followedResponse;
  expect(followed.status()).toBe(200);
  const submitted: Schema<"FollowProviderPlaylist"> = followed
    .request()
    .postDataJSON();
  const receipt: Schema<"PlaylistFollowReceipt"> = await followed.json();
  expect(
    uuid.test(receipt.playlist_id) &&
      uuid.test(submitted.request_id) &&
      receipt.request_id === submitted.request_id,
  ).toBe(true);
  expect(
    submitted.interval === interval &&
      submitted.installation_revision === connection.revision &&
      submitted.playlist_reference === reference,
  ).toBe(true);
  await expect
    .poll(
      () =>
        new URL(page.url()).pathname === `/playlists/${receipt.playlist_id}`,
    )
    .toBe(true);
  const detail = await read<Schema<"PlaylistDetail">>(
    page,
    `/api/v1/playlists/${receipt.playlist_id}`,
  );
  const status = await read<Schema<"PlaylistFollow">>(
    page,
    `/api/v1/playlists/${receipt.playlist_id}/follow`,
  );
  expect(
    detail.playlist.id === receipt.playlist_id &&
      detail.playlist.kind === "followed",
  ).toBe(true);
  expect(
    detail.origin?.provider === connection.manifest.id &&
      detail.origin.installation_id === connection.id &&
      detail.origin.service_id === connection.service_id,
  ).toBe(true);
  expect(
    status.playlist_id === receipt.playlist_id &&
      status.interval === interval &&
      status.status === "ready",
  ).toBe(true);
  expect(
    status.generation === 1 &&
      status.content_revision === 1 &&
      detail.follow_content_revision === 1,
  ).toBe(true);
  expect(status.next_refresh_at - status.last_refreshed_at).toBe(
    interval === "hourly" ? 3600 : 86400,
  );
  await visibleFollow(page, detail);
  return { detail, status };
}

test.describe("isolated live public Spotify Follow", () => {
  test.skip(
    process.env.HITO_SPOTIFY_FOLLOW_LIVE !== "1",
    "Requires explicit disposable Spotify Follow live opt-in",
  );

  test("Hourly and Daily Follows retain the accepted public snapshot and an unchanged explicit refresh publishes nothing", async ({
    page,
    playwright,
    baseURL,
  }, info) => {
    test.setTimeout(240_000);
    let phase = 0;
    let failureCode = 0;
    let cleanupFailed = false;
    let pageErrors = 0;
    let upstream: APIRequestContext | undefined;
    const started = Date.now();
    let origin: string | null = null;
    const requests = {
      follows: 0,
      refreshes: 0,
      imports: 0,
      searches: 0,
      adoptions: 0,
      reviews: 0,
      attachments: 0,
      audio: 0,
      downloads: 0,
      authorization: 0,
      existing_account_auth: 0,
      external: 0,
    };
    const accepted: {
      detail: Schema<"PlaylistDetail">;
      status: Schema<"PlaylistFollow">;
      verified: Awaited<ReturnType<typeof verifySpotifyFollowSnapshot>>;
    }[] = [];
    let sharedRecordings = 0;
    page.on("pageerror", () => pageErrors++);
    page.on("request", (request) => {
      const url = new URL(request.url());
      if (!["http:", "https:"].includes(url.protocol)) return;
      if (url.origin !== origin) requests.external++;
      if (request.resourceType() === "media" || /\/audio$/.test(url.pathname))
        requests.audio++;
      if (/\/downloads?(?:\/|$)/.test(url.pathname)) requests.downloads++;
      if (/\/authorization(?:\/|$)/.test(url.pathname))
        requests.authorization++;
      if (/^\/api\/v1\/(?:login|logout|users)(?:\/|$)/.test(url.pathname))
        requests.existing_account_auth++;
      if (request.method() !== "POST") return;
      if (/\/plugins\/[^/]+\/playlists\/follow$/.test(url.pathname))
        requests.follows++;
      if (/\/playlists\/[^/]+\/follow\/refresh$/.test(url.pathname))
        requests.refreshes++;
      if (/\/plugins\/[^/]+\/playlists\/import$/.test(url.pathname))
        requests.imports++;
      if (/\/plugins\/[^/]+\/search$/.test(url.pathname)) requests.searches++;
      if (/\/plugins\/[^/]+\/recordings$/.test(url.pathname))
        requests.adoptions++;
      if (/\/recordings\/[^/]+\/sources\/review$/.test(url.pathname))
        requests.reviews++;
      if (/\/recordings\/[^/]+\/sources\/attach$/.test(url.pathname))
        requests.attachments++;
    });
    try {
      phase = 1;
      const configuration = spotifyFollowLiveConfiguration();
      try {
        if (!baseURL) throw new SpotifyFollowLiveFailure(1);
        origin = new URL(baseURL).origin;
      } catch {
        throw new SpotifyFollowLiveFailure(1);
      }
      const setup = await read<Schema<"SetupStatus">>(page, "/api/v1/setup");
      expect(setup.needs_setup === true).toBe(true);
      await setUpDisposableAccount(page);
      const beforeActivity = await read<Schema<"Activity">>(
        page,
        "/api/v1/activity",
      );
      upstream = await playwright.request.newContext({
        timeout: 15_000,
        storageState: { cookies: [], origins: [] },
        extraHTTPHeaders: {
          accept: "application/json",
          "accept-encoding": "identity",
          "x-hito-plugin-protocol": "1",
        },
      });

      phase = 2;
      const connection = await connectLiveProvider(
        page,
        configuration.service,
        "Spotify public playlists",
        () => {},
      );
      expect(connection.manifest.id === "org.hito.spotify").toBe(true);
      expect(connection.manifest.capabilities.includes("playlists")).toBe(true);
      expect(
        connection.manifest.capabilities.every(
          (value) => value === "playlists" || value === "artwork",
        ),
      ).toBe(true);
      expect(
        connection.manifest.authorization == null && !connection.credential_set,
      ).toBe(true);
      const settings = await read<Schema<"PluginSettings">>(
        page,
        "/api/v1/plugins",
      );
      expect(
        settings.installations.length === 1 &&
          settings.installations[0].id === connection.id,
      ).toBe(true);

      for (const interval of ["hourly", "daily"] as const) {
        phase = interval === "hourly" ? 3 : 4;
        const { detail, status } = await createFollow(
          page,
          connection,
          configuration.reference,
          interval,
        );
        const verified = await verifySpotifyFollowSnapshot(
          upstream,
          configuration.service,
          detail,
        );
        accepted.push({ detail, status, verified });
      }

      phase = 5;
      const [hourly, daily] = accepted;
      expect(hourly.detail.playlist.id !== daily.detail.playlist.id).toBe(true);
      const hourlyEntries = new Set(
        hourly.detail.entries.map((entry) => entry.entry_id),
      );
      expect(
        daily.detail.entries.every(
          (entry) => !hourlyEntries.has(entry.entry_id),
        ),
      ).toBe(true);
      for (const [provider, recording] of daily.verified.recordings) {
        const existing = hourly.verified.recordings.get(provider);
        if (existing !== undefined) {
          sharedRecordings++;
          expect(recording === existing).toBe(true);
        }
      }
      const beforeFeed = await read<Schema<"SyncPage">>(
        page,
        "/api/v1/sync?cursor=0",
      );
      expect(
        !beforeFeed.has_more &&
          beforeFeed.operations.length === 2 &&
          beforeFeed.operations.every(
            (event) =>
              "publication" in event &&
              event.publication.kind === "playlist_followed",
          ),
      ).toBe(true);
      const refreshResponse = page.waitForResponse(
        (response) =>
          new URL(response.url()).pathname ===
            `/api/v1/playlists/${daily.detail.playlist.id}/follow/refresh` &&
          response.request().method() === "POST",
      );
      await page
        .getByRole("button", { name: "Refresh now", exact: true })
        .click();
      const refreshed = await refreshResponse;
      expect(refreshed.status()).toBe(202);
      const submitted: Schema<"RefreshFollowedPlaylist"> = refreshed
        .request()
        .postDataJSON();
      const receipt: Schema<"PlaylistFollowRefresh"> = await refreshed.json();
      expect(
        receipt.request_id === submitted.request_id &&
          receipt.playlist_id === daily.detail.playlist.id &&
          uuid.test(receipt.job_id),
      ).toBe(true);
      await expect
        .poll(
          async () =>
            (
              await read<Schema<"PlaylistFollow">>(
                page,
                `/api/v1/playlists/${daily.detail.playlist.id}/follow`,
              )
            ).status,
          { timeout: 75_000, intervals: [200, 500, 1000] },
        )
        .toBe("ready");
      const afterStatus = await read<Schema<"PlaylistFollow">>(
        page,
        `/api/v1/playlists/${daily.detail.playlist.id}/follow`,
      );
      if (afterStatus.snapshot !== daily.status.snapshot)
        throw new SpotifyFollowLiveFailure(4);
      expect(
        afterStatus.content_revision === daily.status.content_revision &&
          afterStatus.generation === daily.status.generation &&
          afterStatus.interval === "daily",
      ).toBe(true);
      expect(
        afterStatus.last_refreshed_at >= daily.status.last_refreshed_at &&
          afterStatus.next_refresh_at - afterStatus.last_refreshed_at === 86400,
      ).toBe(true);
      const afterDetail = await read<Schema<"PlaylistDetail">>(
        page,
        `/api/v1/playlists/${daily.detail.playlist.id}`,
      );
      expect(isDeepStrictEqual(afterDetail, daily.detail)).toBe(true);
      const afterFeed = await read<Schema<"SyncPage">>(
        page,
        `/api/v1/sync?cursor=${beforeFeed.cursor}`,
      );
      expect(
        afterFeed.operations.length === 0 &&
          !afterFeed.has_more &&
          afterFeed.cursor === beforeFeed.cursor,
      ).toBe(true);
      await visibleFollow(page, daily.detail);

      phase = 6;
      const recordings = [
        ...new Set(
          accepted.flatMap((value) => [...value.verified.recordings.values()]),
        ),
      ];
      for (let offset = 0; offset < recordings.length; offset += 10) {
        const sources = await Promise.all(
          recordings
            .slice(offset, offset + 10)
            .map((id) =>
              read<Schema<"Source">[]>(
                page,
                `/api/v1/recordings/${id}/sources`,
              ),
            ),
        );
        expect(sources.every((items) => items.length === 0)).toBe(true);
      }
      expect(
        isDeepStrictEqual(
          await read<Schema<"Activity">>(page, "/api/v1/activity"),
          beforeActivity,
        ),
      ).toBe(true);
      expect(await page.locator("audio").getAttribute("src")).toBeNull();
      expect(requests.follows === 2 && requests.refreshes === 1).toBe(true);
      expect(
        Object.entries(requests).every(
          ([key, count]) =>
            key === "follows" || key === "refreshes" || count === 0,
        ),
      ).toBe(true);
      expect(pageErrors).toBe(0);
      const evidence = {
        event: "spotify_follow_live_verified",
        accepted_entries: accepted.map((value) => value.verified.entries),
        accepted_pages: accepted.map((value) => value.verified.pages),
        duplicate_occurrences: accepted.map(
          (value) => value.verified.duplicates,
        ),
        shared_recordings: sharedRecordings,
        unchanged_content_revision: true,
        unchanged_collection_cursor: true,
        unchanged_detail_and_activity: true,
        ...requests,
        page_errors: pageErrors,
        elapsed_ms: Date.now() - started,
      };
      await info.attach("spotify-follow-live-numerical-evidence", {
        body: Buffer.from(JSON.stringify(evidence)),
        contentType: "application/json",
      });
      console.info(JSON.stringify(evidence));
    } catch (error) {
      failureCode = error instanceof SpotifyFollowLiveFailure ? error.code : 5;
      console.info(
        JSON.stringify({
          event: "spotify_follow_live_failed",
          phase,
          failure_code: failureCode,
          upstream_snapshot_changed: failureCode === 4,
          accepted_counts: accepted.map((value) => value.verified.entries),
          ...requests,
          page_errors: pageErrors,
        }),
      );
    } finally {
      await upstream?.dispose().catch(() => {
        cleanupFailed = true;
      });
      await page.close().catch(() => {
        cleanupFailed = true;
      });
    }
    if (failureCode)
      throw new Error(
        `Spotify Follow live acceptance failed at phase ${phase} with classification ${failureCode}`,
      );
    if (cleanupFailed)
      throw new Error("The disposable live browser cleanup failed");
  });
});
