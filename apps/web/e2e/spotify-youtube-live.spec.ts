import { expect, test } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import { liveAudioRequestDiagnostics } from "./fixtures/youtube-audio-network";
import {
  liveControlBounds,
  liveMediaDiagnostics,
} from "./fixtures/youtube-live";
import {
  connectLiveProvider,
  setUpSpotifyYouTubeLiveAccount,
  searchLiveCandidate,
  reviewLiveCandidate,
  spotifyYouTubeConfiguration,
  SpotifyYouTubeLiveFailure,
} from "./fixtures/spotify-youtube-live";
import {
  assertAttachedCollection,
  assertPlaybackPreservesCollection,
  assertReviewPreservesPlaylist,
  editPrivateDetailsAndCaptureBaseline,
  inspectCopiedPlaylist,
  inspectImportedRecording,
  readSpotifyYouTube,
  uuid,
} from "./fixtures/spotify-youtube-collection";
import {
  audioPath,
  installSpotifyYouTubeMediaDiagnostics,
  spotifyYouTubeMediaEvents,
  verifySpotifyYouTubePlayback,
} from "./fixtures/spotify-youtube-playback";
import { openLibrarySection } from "./fixtures/library-sections";

test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});

test.describe("isolated Spotify copy with reviewed YouTube audio", () => {
  test.skip(
    process.env.HITO_YOUTUBE_LIVE !== "1",
    "Requires explicit live-image opt-in",
  );

  test("a real imported playlist retains Spotify identity while reviewed YouTube audio plays and seeks", async ({
    page,
    baseURL,
    browserName,
  }, info) => {
    test.setTimeout(180_000);
    let phase = 0;
    let failure: string | null = null;
    let cleanupFailed = false;
    let pageErrors = 0;
    let externalRequests = 0;
    let invalidMediaRequests = 0;
    let imports = 0;
    let reviews = 0;
    let attachments = 0;
    let adoptions = 0;
    const services: Schema<"PluginService">[] = [];
    const started = Date.now();
    const origin = baseURL ? new URL(baseURL).origin : null;
    const headers = { "x-hito-request": "1", origin: origin ?? "" };
    const network = liveAudioRequestDiagnostics(
      page,
      origin,
      () => phase,
      started,
    );
    page.on("pageerror", () => pageErrors++);
    page.on("request", (request) => {
      const url = new URL(request.url());
      if (!["http:", "https:"].includes(url.protocol)) return;
      if (url.origin !== origin) externalRequests++;
      if (
        request.resourceType() === "media" &&
        (url.origin !== origin ||
          !audioPath.test(url.pathname) ||
          url.search ||
          url.hash)
      )
        invalidMediaRequests++;
      if (request.method() !== "POST") return;
      if (/\/plugins\/[^/]+\/playlists\/import$/.test(url.pathname)) imports++;
      if (/\/recordings\/[^/]+\/sources\/review$/.test(url.pathname)) reviews++;
      if (/\/recordings\/[^/]+\/sources\/attach$/.test(url.pathname))
        attachments++;
      if (/\/plugins\/[^/]+\/recordings$/.test(url.pathname)) adoptions++;
    });
    try {
      phase = 1;
      if (!origin)
        throw new SpotifyYouTubeLiveFailure(
          "The disposable browser server must be configured",
        );
      const configuration = spotifyYouTubeConfiguration();
      await installSpotifyYouTubeMediaDiagnostics(page, started);
      const capability = await page.evaluate(() =>
        document
          .createElement("audio")
          .canPlayType('audio/mp4; codecs="mp4a.40.2"'),
      );
      if (!capability)
        throw new SpotifyYouTubeLiveFailure(
          "This browser does not support the required AAC codec",
        );
      await setUpSpotifyYouTubeLiveAccount(page);
      const initialPlaylists = await readSpotifyYouTube<Schema<"Playlist">[]>(
        page,
        "/api/v1/playlists",
      );

      phase = 2;
      const spotify = await connectLiveProvider(
        page,
        configuration.spotify,
        "Spotify public playlists",
        (service) => services.push(service),
      );
      expect(
        spotify.manifest.capabilities.includes("playlists") &&
          !spotify.manifest.capabilities.includes("sources"),
      ).toBe(true);
      const youtube = await connectLiveProvider(
        page,
        configuration.youtube,
        "YouTube",
        (service) => services.push(service),
      );
      expect(
        youtube.manifest.capabilities.includes("search") &&
          youtube.manifest.capabilities.includes("sources"),
      ).toBe(true);

      phase = 3;
      await openLibrarySection(page, "Playlists");
      const form = page.getByRole("region", {
        name: "Import a playlist",
        exact: true,
      });

      await form
        .getByRole("textbox", { name: "Playlist link or identifier" })
        .fill(configuration.playlist);
      const importedResponse = page.waitForResponse(
        (response) =>
          new URL(response.url()).pathname ===
            `/api/v1/plugins/${spotify.id}/playlists/import` &&
          response.request().method() === "POST",
        { timeout: 60_000 },
      );
      await form
        .getByRole("button", { name: "Import a copy", exact: true })
        .click();
      const imported = await importedResponse;
      if (imported.status() !== 200)
        throw new SpotifyYouTubeLiveFailure(
          `Playlist copy returned HTTP ${imported.status()}`,
        );
      const receipt: Schema<"PlaylistImportReceipt"> = await imported.json();
      expect(uuid.test(receipt.playlist_id)).toBe(true);
      await expect
        .poll(
          () =>
            new URL(page.url()).pathname ===
            `/playlists/${receipt.playlist_id}`,
        )
        .toBe(true);
      const { copied, recording } = await inspectCopiedPlaylist(
        page,
        receipt,
        spotify,
      );
      const table = page.getByRole("table", {
        name: "Playlist entries",
        exact: true,
      });
      await expect(table.getByRole("row")).toHaveCount(
        copied.entries.length + 1,
      );
      const originalRow = table
        .getByRole("row")
        .filter({ has: page.locator(`a[href="/recordings/${recording}"]`) })
        .first();
      await expect(
        originalRow.getByRole("button", { name: /^Play .+/ }),
      ).toBeDisabled();
      const original = await inspectImportedRecording(page, recording, spotify);

      phase = 4;
      const personalTitle = "Live attachment personal track title";
      const personalArtist = "Live attachment personal artist";
      const baseline = await editPrivateDetailsAndCaptureBaseline(page, {
        receipt,
        recording,
        initialPlaylists,
        personalTitle,
        personalArtist,
        headers,
      });
      await originalRow.locator(`a[href="/recordings/${recording}"]`).click();
      await expect(page.getByLabel("Track title", { exact: true })).toHaveValue(
        personalTitle,
      );
      expect(
        await page
          .locator(".metadata-inheritance")
          .evaluateAll(
            (elements, source) =>
              elements.some((element) =>
                element.textContent?.includes(source.title),
              ) &&
              elements.some((element) =>
                element.textContent?.includes(source.artist),
              ),
            { title: original.title, artist: original.artist },
          ),
      ).toBe(true);

      phase = 5;
      const { panel, results, selection, result } = await searchLiveCandidate(
        page,
        {
          youtube,
          original,
        },
      );
      expect(attachments + adoptions).toBe(0);

      phase = 6;
      const { review, comparison } = await reviewLiveCandidate(page, {
        panel,
        result,
        recording,
        selection,
        original,
        personalTitle,
        personalArtist,
      });
      await assertReviewPreservesPlaylist(
        page,
        receipt,
        baseline.beforePlaylist,
      );
      expect(attachments + adoptions).toBe(0);
      const confirm = comparison.getByRole("button", {
        name: "Use this source",
        exact: true,
      });
      await liveControlBounds(page, confirm, true);

      phase = 7;
      const attachedResponse = page.waitForResponse(
        (response) =>
          new URL(response.url()).pathname ===
            `/api/v1/recordings/${recording}/sources/attach` &&
          response.request().method() === "POST",
      );
      await confirm.click();
      const attached = await attachedResponse;
      if (attached.status() !== 200)
        throw new SpotifyYouTubeLiveFailure(
          `Reviewed source attachment returned HTTP ${attached.status()}`,
        );
      const attachBody: Schema<"AttachReviewedProviderSource"> = attached
        .request()
        .postDataJSON();
      expect(
        attachBody.candidate.installation_id === youtube.id &&
          attachBody.candidate.installation_revision === youtube.revision &&
          attachBody.candidate.provider_recording_id ===
            selection.candidate.id &&
          attachBody.review_stamp === review.review_stamp,
      ).toBe(true);
      const retained: Schema<"Recording"> = await attached.json();
      expect(
        retained.id === recording &&
          retained.available &&
          retained.title === personalTitle &&
          retained.artist === personalArtist,
      ).toBe(true);
      await expect(
        panel.getByText("Source added to this track.", { exact: true }),
      ).toBeVisible();
      const { afterPlaylist, sources } = await assertAttachedCollection(page, {
        receipt,
        recording,
        baseline,
        youtube,
        selection,
      });

      const { first, target, afterSeek, navigated } =
        await verifySpotifyYouTubePlayback(page, {
          receipt,
          recording,
          beforePlaylist: baseline.beforePlaylist,
          afterPlaylist,
          sources,
          original,
          origin,
          setPhase: (value) => {
            phase = value;
          },
        });
      await assertPlaybackPreservesCollection(
        page,
        receipt,
        recording,
        afterPlaylist,
      );
      expect(imports).toBe(1);
      expect(reviews).toBe(1);
      expect(attachments).toBe(1);
      expect(
        adoptions + pageErrors + externalRequests + invalidMediaRequests,
      ).toBe(0);
      await network.settleDiagnostics();
      const audioNetwork = network.summary(browserName);
      expect(
        audioNetwork.audio_http_failures +
          audioNetwork.audio_framing_failures +
          audioNetwork.audio_unexpected_request_failures +
          audioNetwork.audio_observation_overflow,
      ).toBe(0);
      expect(audioNetwork.audio_request_failures).toBe(
        audioNetwork.audio_browser_cancellations +
          audioNetwork.audio_unexpected_request_failures,
      );
      expect(audioNetwork.audio_responses).toBeGreaterThan(0);
      const evidence = {
        event: "spotify_youtube_live_browser_verified",
        browser_family:
          browserName === "webkit" ? 3 : browserName === "firefox" ? 2 : 1,
        entries: copied.entries.length,
        other_unavailable_entries: afterPlaylist.entries.filter(
          (entry) => entry.recording_id !== recording,
        ).length,
        candidate_count: results.items.length,
        qualifying_candidates: selection.qualifying,
        candidate_duration_delta_ms: Math.abs(
          review.candidate.duration_ms! - original.duration_ms,
        ),
        private_overrides_preserved: 1,
        canonical_identity_preserved: 1,
        spotify_metadata_owner_preserved: 1,
        playlist_identity_order_preserved: 1,
        imports,
        reviews,
        attachments,
        adoptions,
        first_time: first.time,
        seek_target: target,
        seek_time: afterSeek.time,
        navigated_time: navigated.time,
        ready_state: navigated.ready,
        media_error: navigated.error,
        external_requests: externalRequests,
        invalid_media_requests: invalidMediaRequests,
        page_errors: pageErrors,
        ...audioNetwork,
        audio_requests: network.snapshot(),
        media_events: await spotifyYouTubeMediaEvents(page),
        elapsed_ms: Date.now() - started,
      };
      await info.attach("spotify-youtube-live-numerical-evidence", {
        body: Buffer.from(JSON.stringify(evidence)),
        contentType: "application/json",
      });
      console.info(JSON.stringify(evidence));
      await page
        .getByRole("contentinfo", { name: "Audio player" })
        .getByRole("button", { name: "Pause", exact: true })
        .click();
    } catch (error) {
      failure = `Spotify to YouTube live workflow failed at phase ${phase}: ${error instanceof SpotifyYouTubeLiveFailure ? error.message : "UI, identity or media assertion failed"}`;
      let media: Record<string, number> = { diagnostic_available: 0 };
      let mediaEvents: Record<string, number>[] = [];
      try {
        await network.settleDiagnostics();
        media = {
          diagnostic_available: 1,
          ...(await liveMediaDiagnostics(page)),
        };
        mediaEvents = await spotifyYouTubeMediaEvents(page);
      } catch {
        /* Keep the original failure private. */
      }
      console.info(
        JSON.stringify({
          event: "spotify_youtube_live_browser_failed",
          phase,
          imports,
          reviews,
          attachments,
          adoptions,
          page_errors: pageErrors,
          external_requests: externalRequests,
          invalid_media_requests: invalidMediaRequests,
          ...network.summary(browserName),
          audio_requests: network.snapshot(),
          media_events: mediaEvents,
          ...media,
        }),
      );
    } finally {
      network.markCleanup();
      for (const service of services.reverse()) {
        try {
          const removed = await page.request.delete(
            `/api/v1/plugins/services/${service.id}`,
            { headers, data: { revision: service.revision } },
          );
          if (removed.status() !== 204) cleanupFailed = true;
        } catch {
          cleanupFailed = true;
        }
      }
      await page.close().catch(() => {
        cleanupFailed = true;
      });
    }
    if (failure) throw new Error(failure);
    if (cleanupFailed)
      throw new Error(
        "The disposable live services could not be withdrawn during cleanup",
      );
  });
});
