import { expect, test as baseTest, type Route } from "@playwright/test";
import type { components } from "../src/generated";
import { preparePlaylistDownload } from "./fixtures/playlist-download";
import { providerFixture } from "./fixtures/provider";
import { openLibrarySection } from "./fixtures/library-sections";

type Schema = components["schemas"];
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const optedIn = process.env.HITO_PLAYLIST_DOWNLOAD_E2E === "1";
const test = baseTest.extend<{ playlistDownloadPrivacy: void }>({
  playlistDownloadPrivacy: [
    async ({}, provide) => {
      if (!optedIn) {
        await provide();
        return;
      }
      const previous = process.env.PLAYWRIGHT_NO_COPY_PROMPT;
      process.env.PLAYWRIGHT_NO_COPY_PROMPT = "1";
      try {
        await provide();
      } finally {
        if (previous === undefined)
          delete process.env.PLAYWRIGHT_NO_COPY_PROMPT;
        else process.env.PLAYWRIGHT_NO_COPY_PROMPT = previous;
      }
    },
    { auto: true },
  ],
});

test.use({ trace: "off", screenshot: "off", video: "off" });

test.describe("explicit persistent playlist download", () => {
  test.skip(
    !optedIn,
    "Requires an opted-in fresh disposable playlist download server",
  );

  test("failed duplicates recover through an exact request retry and play from their shared server copy", async ({
    page,
    context,
    baseURL,
  }) => {
    test.setTimeout(90_000);
    const creations: string[] = [];
    const removals: string[] = [];
    let provider: Awaited<ReturnType<typeof providerFixture>> | null = null;
    let phase = "configuration";
    let failedPhase: string | null = null;
    let downloadPath = "";
    let individualWrites = 0;
    let browserAudioRequests = 0;
    let statusChecks = 0;
    let pageErrors = 0;
    let callbackFailures = 0;
    let cleanupFailures = 0;
    const closePrivatePage = async () => {
      if (page.isClosed()) return;
      try {
        await page.close({ runBeforeUnload: false });
      } catch {
        cleanupFailures++;
        try {
          await context.close();
        } catch {
          cleanupFailures++;
        }
      }
    };
    try {
      if (!baseURL)
        throw new Error("The disposable server URL must be configured");
      const origin = new URL(baseURL).origin;
      phase = "provider_fixture";
      const fixture = await providerFixture("Download");
      provider = fixture;
      page.on("pageerror", () => pageErrors++);
      context.on("request", (request) => {
        try {
          const url = new URL(request.url());
          if (url.origin !== origin) return;
          if (/^\/api\/v1\/sources\/[0-9a-f-]+\/audio$/.test(url.pathname))
            browserAudioRequests++;
          if (
            request.method() === "POST" &&
            /^\/api\/v1\/recordings\/[0-9a-f-]+\/download$/.test(url.pathname)
          )
            individualWrites++;
          if (url.pathname === downloadPath) {
            if (request.method() === "POST") {
              const body: Schema["DownloadRequest"] = request.postDataJSON();
              creations.push(body.request_id);
            } else if (request.method() === "GET") statusChecks++;
          }
          if (
            request.method() === "DELETE" &&
            /^\/api\/v1\/downloads\/[0-9a-f-]+$/.test(url.pathname)
          )
            removals.push(url.pathname);
        } catch {
          callbackFailures++;
        }
      });
      phase = "setup";
      const library = await preparePlaylistDownload(page, baseURL, fixture);
      downloadPath = library.downloadPath;
      fixture.failSource(fixture.source, 503);
      const details = `/playlists/${library.playlist}`;
      await page.goto(details);
      const section = page.getByRole("region", {
        name: "Playlist download",
        exact: true,
      });
      const audio = page.locator("audio");
      const download = section.getByRole("button", {
        name: "Download",
        exact: true,
      });
      await expect(download).toBeEnabled();
      expect((await page.request.get(downloadPath)).status()).toBe(404);
      await library.assertNoIndividualDownload(page);
      expect(creations.length).toBe(0);
      expect(fixture.requests.length).toBe(0);
      expect(browserAudioRequests).toBe(0);
      expect(await audio.getAttribute("src")).toBeNull();
      expect(
        await audio.evaluate((element: HTMLAudioElement) => element.paused),
      ).toBe(true);

      phase = "initial_download";
      const initialResponse = page
        .waitForResponse(
          (response) =>
            new URL(response.url()).pathname === downloadPath &&
            response.request().method() === "POST",
        )
        .catch(() => null);
      await download.click();
      const initial = await initialResponse;
      if (initial === null) throw new Error("The initial response must arrive");
      expect(initial.status()).toBe(202);
      const accepted: Schema["PlaylistDownloadReceipt"] = await initial.json();
      expect(
        uuid.test(accepted.retention_id) &&
          accepted.playlist_id === library.playlist,
      ).toBe(true);
      expect(
        creations.length === 1 &&
          creations[0] === accepted.request_id &&
          uuid.test(creations[0]),
      ).toBe(true);
      phase = "terminal_failure";
      await expect(
        section.getByText("2 entries could not be downloaded.", {
          exact: true,
        }),
      ).toBeVisible({ timeout: 20_000 });
      const failed = await library.state(page, accepted.retention_id, "failed");
      expect(
        failed.entries.items.every(
          (item) => item.error === "transfer_failed" && item.source_id == null,
        ),
      ).toBe(true);
      expect(fixture.requests.length).toBe(3);
      expect(browserAudioRequests).toBe(0);
      expect(await audio.getAttribute("src")).toBeNull();
      expect(
        await audio.evaluate((element: HTMLAudioElement) => element.paused),
      ).toBe(true);
      await library.assertNoIndividualDownload(page);
      await library.assertRetained(page);

      phase = "retry_lost_acknowledgement";
      fixture.failSource(fixture.source, 0);
      let committed = false;
      const lostReceipts: Schema["PlaylistDownloadReceipt"][] = [];
      const routePattern = `**${downloadPath}`;
      const loseAcknowledgement = async (route: Route) => {
        try {
          if (route.request().method() !== "POST" || committed) {
            await route.continue();
            return;
          }
          const actual = await route.fetch();
          expect(actual.status()).toBe(202);
          lostReceipts.push(await actual.json());
          committed = true;
          await actual.dispose();
          await route.abort("failed");
        } catch {
          callbackFailures++;
          try {
            await route.abort("failed");
          } catch {
            callbackFailures++;
          }
        }
      };
      await page.route(routePattern, loseAcknowledgement);
      await section
        .getByRole("button", { name: "Retry failed downloads", exact: true })
        .click();
      const recoverRequest = section.getByRole("button", {
        name: "Retry download request",
        exact: true,
      });
      await expect(recoverRequest).toBeEnabled();
      expect(committed).toBe(true);
      expect(
        creations.length === 2 &&
          creations[1] !== creations[0] &&
          uuid.test(creations[1]),
      ).toBe(true);
      expect(lostReceipts.length).toBe(1);
      const lostReceipt = lostReceipts[0];
      expect(
        lostReceipt.retention_id === accepted.retention_id &&
          lostReceipt.playlist_id === library.playlist &&
          lostReceipt.request_id === creations[1],
      ).toBe(true);
      await page.unroute(routePattern, loseAcknowledgement);
      phase = "retry_confirmation";
      const replayResponse = page
        .waitForResponse(
          (response) =>
            new URL(response.url()).pathname === downloadPath &&
            response.request().method() === "POST",
        )
        .catch(() => null);
      const checksBeforeRecovery = statusChecks;
      const retryStarted = Date.now();
      await recoverRequest.click();
      const replay = await replayResponse;
      if (replay === null) throw new Error("The replay response must arrive");
      expect(replay.status()).toBe(202);
      const replayed: Schema["PlaylistDownloadReceipt"] = await replay.json();
      expect(
        replayed.request_id === creations[1] &&
          replayed.retention_id === accepted.retention_id &&
          replayed.playlist_id === library.playlist,
      ).toBe(true);
      expect(creations.length === 3 && creations[2] === creations[1]).toBe(
        true,
      );
      phase = "natural_readiness";
      await expect(section.getByRole("status")).toHaveText(
        "2 of 2 entries saved.",
        { timeout: 36_000 },
      );
      test.info().annotations.push({
        type: "playlist_download_retry_observation",
        description: JSON.stringify({
          ready_ms: Date.now() - retryStarted,
          checks: statusChecks - checksBeforeRecovery,
        }),
      });
      const ready = await library.state(page, accepted.retention_id, "ready");
      const source = ready.entries.items[0].source_id;
      expect(
        typeof source === "string" &&
          uuid.test(source) &&
          ready.entries.items.every((item) => item.source_id === source),
      ).toBe(true);
      if (typeof source !== "string")
        throw new Error("A shared managed source must be present");
      expect(fixture.requests.length).toBe(4);
      expect(browserAudioRequests).toBe(0);
      expect(individualWrites).toBe(0);
      expect(await audio.getAttribute("src")).toBeNull();
      expect(
        await audio.evaluate((element: HTMLAudioElement) => element.paused),
      ).toBe(true);
      await library.assertNoIndividualDownload(page);
      await library.assertRetained(page);

      phase = "navigation_reload";
      await openLibrarySection(page, "History");
      await page.goto(details);
      await expect(section.getByRole("status")).toHaveText(
        "2 of 2 entries saved.",
      );
      await page.reload();
      await expect(section.getByRole("status")).toHaveText(
        "2 of 2 entries saved.",
      );
      const reloaded = await library.state(
        page,
        accepted.retention_id,
        "ready",
      );
      expect(
        reloaded.entries.items.every((item) => item.source_id === source),
      ).toBe(true);
      expect(creations.length).toBe(3);
      expect(browserAudioRequests).toBe(0);

      phase = "managed_playback";
      fixture.failSource(fixture.source, 503);
      await library.assertOwnedAudio(page, source);
      const providerReads = fixture.requests.length;
      expect(providerReads).toBe(4);
      await page
        .getByRole("button", { name: "Play playlist", exact: true })
        .click();
      await expect
        .poll(() =>
          audio.evaluate((element: HTMLAudioElement) => element.currentTime),
        )
        .toBeGreaterThan(0.2);
      const playing = await audio.evaluate(
        (element: HTMLAudioElement) => element.src,
      );
      expect(
        new URL(playing).origin === origin &&
          new URL(playing).pathname === `/api/v1/sources/${source}/audio`,
      ).toBe(true);
      expect(browserAudioRequests).toBeGreaterThan(0);
      expect(fixture.requests.length).toBe(providerReads);
      await page
        .getByRole("contentinfo", { name: "Audio player", exact: true })
        .getByRole("button", { name: "Pause", exact: true })
        .click();
      await expect
        .poll(() =>
          audio.evaluate((element: HTMLAudioElement) => element.paused),
        )
        .toBe(true);

      phase = "removal";
      const removalPath = `/api/v1/downloads/${accepted.retention_id}`;
      const removedResponse = page
        .waitForResponse(
          (response) =>
            new URL(response.url()).pathname === removalPath &&
            response.request().method() === "DELETE",
        )
        .catch(() => null);
      await section
        .getByRole("button", { name: "Remove download", exact: true })
        .click();
      const removed = await removedResponse;
      if (removed === null) throw new Error("The removal response must arrive");
      expect(removed.status()).toBe(204);
      await expect(
        section.getByRole("button", { name: "Download", exact: true }),
      ).toBeEnabled();
      expect((await page.request.get(downloadPath)).status()).toBe(404);
      await expect
        .poll(
          async () =>
            (
              await page.request.get(`/api/v1/sources/${source}/audio`)
            ).status(),
          { timeout: 10_000 },
        )
        .toBe(404);
      expect(removals.length === 1 && removals[0] === removalPath).toBe(true);
      expect(creations.length).toBe(3);
      expect(individualWrites).toBe(0);
      expect(fixture.requests.length).toBe(4);
      await library.assertNoIndividualDownload(page);
      await library.assertRetained(page);
      expect(pageErrors).toBe(0);
      expect(callbackFailures).toBe(0);
    } catch {
      failedPhase = phase;
    } finally {
      if (failedPhase !== null) await closePrivatePage();
      try {
        await provider?.close();
      } catch {
        cleanupFailures++;
        failedPhase ??= "provider_cleanup";
      }
      if (callbackFailures > 0) failedPhase ??= "browser_callback";
      if (failedPhase !== null) await closePrivatePage();
    }
    if (failedPhase !== null) {
      const failure = new Error(
        `Playlist download check failed: ${failedPhase}; counts=${JSON.stringify(
          {
            creations: creations.length,
            removals: removals.length,
            provider_requests: provider?.requests.length ?? 0,
            individual_writes: individualWrites,
            browser_audio_requests: browserAudioRequests,
            status_checks: statusChecks,
            page_errors: pageErrors,
            callback_failures: callbackFailures,
            cleanup_failures: cleanupFailures,
          },
        )}`,
      );
      failure.stack = failure.message;
      throw failure;
    }
  });
});
