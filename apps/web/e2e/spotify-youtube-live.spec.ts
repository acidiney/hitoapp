import { randomUUID } from "node:crypto";
import { expect, test } from "@playwright/test";
import type { components } from "../src/generated";
import { liveAudioRequestDiagnostics } from "./fixtures/youtube-audio-network";
import {
  liveAudioState,
  liveControlBounds,
  liveMediaDiagnostics,
  signInLiveAccount,
} from "./fixtures/youtube-live";
import {
  connectLiveProvider,
  conservativeCandidate,
  installSpotifyYouTubeMediaDiagnostics,
  readSpotifyYouTube,
  retainedImportedPlaylist,
  spotifyYouTubeConfiguration,
  spotifyYouTubeMediaEvents,
  SpotifyYouTubeLiveFailure,
} from "./fixtures/spotify-youtube-live";

type Schema = components["schemas"];
const audioPath =
  /^\/api\/v1\/sources\/[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}\/audio$/;
const uuid = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;
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
    const services: Schema["PluginService"][] = [];
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
      await signInLiveAccount(page);
      const initialPlaylists = await readSpotifyYouTube<Schema["Playlist"][]>(
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
      await page
        .getByRole("navigation", { name: "Main navigation" })
        .getByRole("link", { name: "Playlists", exact: true })
        .click();
      const form = page.getByRole("region", {
        name: "Import a playlist",
        exact: true,
      });
      await form
        .getByRole("combobox", { name: "Playlist service" })
        .selectOption(spotify.id);
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
      const receipt: Schema["PlaylistImportReceipt"] = await imported.json();
      expect(uuid.test(receipt.playlist_id)).toBe(true);
      await expect
        .poll(
          () =>
            new URL(page.url()).pathname ===
            `/playlists/${receipt.playlist_id}`,
        )
        .toBe(true);
      const copied = await readSpotifyYouTube<Schema["PlaylistDetail"]>(
        page,
        `/api/v1/playlists/${receipt.playlist_id}`,
      );
      expect(
        copied.playlist.id === receipt.playlist_id &&
          copied.playlist.kind === "local",
      ).toBe(true);
      expect(
        copied.origin?.provider === spotify.manifest.id &&
          copied.origin?.installation_id === spotify.id &&
          copied.origin?.attribution.label === "Spotify",
      ).toBe(true);
      expect(copied.entries.length).toBeGreaterThan(1);
      expect(copied.entries.length).toBeLessThanOrEqual(500);
      expect(copied.entries.every((entry) => !entry.available)).toBe(true);
      expect(new Set(copied.entries.map((entry) => entry.entry_id)).size).toBe(
        copied.entries.length,
      );
      const chosen = copied.entries.find(
        (entry) =>
          entry.recording_id &&
          entry.origin?.kind === "recording" &&
          entry.duration_ms > 10_000 &&
          entry.duration_ms <= 600_000,
      );
      if (!chosen?.recording_id)
        throw new SpotifyYouTubeLiveFailure(
          "The copied playlist has no bounded saved track for explicit review",
        );
      const recording = chosen.recording_id;
      expect(
        uuid.test(recording) &&
          recording !== chosen.origin?.provider_recording_id,
      ).toBe(true);
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
      const originalMetadata = await readSpotifyYouTube<
        Schema["RecordingMetadata"]
      >(page, `/api/v1/recordings/${recording}/metadata`);
      const original = originalMetadata.selected_observation;
      if (
        !original ||
        original.origin !== "provider_response" ||
        original.provider !== spotify.manifest.id ||
        original.plugin_installation_id !== spotify.id
      )
        throw new SpotifyYouTubeLiveFailure(
          "The saved track must retain its Spotify metadata owner",
        );
      expect(
        (
          await readSpotifyYouTube<Schema["Source"][]>(
            page,
            `/api/v1/recordings/${recording}/sources`,
          )
        ).length,
      ).toBe(0);

      phase = 4;
      const personalTitle = "Live attachment personal track title";
      const personalArtist = "Live attachment personal artist";
      const edited = await page.request.post("/api/v1/sync", {
        headers,
        data: {
          device_id: "spotify-youtube-live",
          operations: [
            {
              id: randomUUID(),
              action: {
                kind: "recording_title_set",
                recording_id: recording,
                title: personalTitle,
              },
            },
            {
              id: randomUUID(),
              action: {
                kind: "recording_artist_set",
                recording_id: recording,
                artist: personalArtist,
              },
            },
          ],
        } satisfies Schema["SyncBatch"],
      });
      if (edited.status() !== 200)
        throw new SpotifyYouTubeLiveFailure(
          `Owned private edits returned HTTP ${edited.status()}`,
        );
      const beforeMetadata = await readSpotifyYouTube<
        Schema["RecordingMetadata"]
      >(page, `/api/v1/recordings/${recording}/metadata`);
      const beforePlaylist = await readSpotifyYouTube<Schema["PlaylistDetail"]>(
        page,
        `/api/v1/playlists/${receipt.playlist_id}`,
      );
      const beforeLists = await readSpotifyYouTube<Schema["Playlist"][]>(
        page,
        "/api/v1/playlists",
      );
      const beforeCatalog = await readSpotifyYouTube<Schema["RecordingPage"]>(
        page,
        "/api/v1/recordings?limit=1",
      );
      expect(beforeLists.length).toBe(initialPlaylists.length + 1);
      expect(
        beforeMetadata.title.personal === personalTitle &&
          beforeMetadata.artist.personal === personalArtist,
      ).toBe(true);
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
      const panel = page.getByRole("region", {
        name: "Add an audio source",
        exact: true,
      });
      await panel
        .getByRole("button", { name: "Find a source", exact: true })
        .click();
      await panel
        .getByRole("combobox", { name: "Music service", exact: true })
        .selectOption(youtube.id);
      const query = `${original.title} ${original.artist}`.trim();
      if (query.length > 512)
        throw new SpotifyYouTubeLiveFailure(
          "The original metadata exceeds the bounded search input",
        );
      await panel
        .getByRole("searchbox", { name: "Search music", exact: true })
        .fill(query);
      const searchedResponse = page.waitForResponse(
        (response) =>
          new URL(response.url()).pathname ===
            `/api/v1/plugins/${youtube.id}/search` &&
          response.request().method() === "POST",
      );
      await panel.getByRole("button", { name: "Search", exact: true }).click();
      const searched = await searchedResponse;
      if (searched.status() !== 200)
        throw new SpotifyYouTubeLiveFailure(
          `Source search returned HTTP ${searched.status()}`,
        );
      const results: Schema["SearchPage"] = await searched.json();
      const selection = conservativeCandidate(original, results.items);
      const result = panel
        .getByRole("list", { name: "Source search results" })
        .locator(":scope > li")
        .nth(selection.index);
      await expect(result.getByRole("heading")).toBeVisible();
      expect(
        await result
          .getByRole("heading")
          .evaluate(
            (element, title) => element.textContent === title,
            selection.candidate.title,
          ),
      ).toBe(true);
      expect(attachments + adoptions).toBe(0);

      phase = 6;
      const reviewedResponse = page.waitForResponse(
        (response) =>
          new URL(response.url()).pathname ===
            `/api/v1/recordings/${recording}/sources/review` &&
          response.request().method() === "POST",
      );
      await result
        .getByRole("button", { name: "Review match", exact: true })
        .click();
      const reviewed = await reviewedResponse;
      if (reviewed.status() !== 200)
        throw new SpotifyYouTubeLiveFailure(
          `Source review returned HTTP ${reviewed.status()}`,
        );
      const review: Schema["ProviderSourceReview"] = await reviewed.json();
      expect(review.candidate.id === selection.candidate.id).toBe(true);
      conservativeCandidate(original, [review.candidate]);
      const comparison = panel.getByRole("region", {
        name: "Review source match",
        exact: true,
      });
      await expect(
        comparison.getByRole("heading", { name: "Saved track", exact: true }),
      ).toBeVisible();
      await expect(
        comparison.getByRole("heading", {
          name: "Candidate source",
          exact: true,
        }),
      ).toBeVisible();
      expect(
        await comparison
          .locator(":scope > div")
          .nth(0)
          .evaluate(
            (element, saved) =>
              element.textContent?.includes(saved.title) &&
              element.textContent?.includes(saved.artist),
            { title: personalTitle, artist: personalArtist },
          ),
      ).toBe(true);
      expect(
        await comparison
          .locator(":scope > div")
          .nth(1)
          .evaluate(
            (element, candidate) =>
              element.textContent?.includes(candidate.title) &&
              element.textContent?.includes(candidate.artist),
            { title: review.candidate.title, artist: review.candidate.artist },
          ),
      ).toBe(true);
      expect(
        JSON.stringify(
          await readSpotifyYouTube<Schema["PlaylistDetail"]>(
            page,
            `/api/v1/playlists/${receipt.playlist_id}`,
          ),
        ) === JSON.stringify(beforePlaylist),
      ).toBe(true);
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
      const attachBody: Schema["AttachReviewedProviderSource"] = attached
        .request()
        .postDataJSON();
      expect(
        attachBody.candidate.installation_id === youtube.id &&
          attachBody.candidate.installation_revision === youtube.revision &&
          attachBody.candidate.provider_recording_id ===
            selection.candidate.id &&
          attachBody.review_stamp === review.review_stamp,
      ).toBe(true);
      const retained: Schema["Recording"] = await attached.json();
      expect(
        retained.id === recording &&
          retained.available &&
          retained.title === personalTitle &&
          retained.artist === personalArtist,
      ).toBe(true);
      await expect(
        panel.getByText("Source added to this track.", { exact: true }),
      ).toBeVisible();
      const afterMetadata = await readSpotifyYouTube<
        Schema["RecordingMetadata"]
      >(page, `/api/v1/recordings/${recording}/metadata`);
      expect(
        afterMetadata.recording_id === recording &&
          JSON.stringify(afterMetadata.title) ===
            JSON.stringify(beforeMetadata.title) &&
          JSON.stringify(afterMetadata.artist) ===
            JSON.stringify(beforeMetadata.artist) &&
          JSON.stringify(afterMetadata.selected_observation) ===
            JSON.stringify(beforeMetadata.selected_observation),
      ).toBe(true);
      expect(
        afterMetadata.observations.items.some(
          (item) =>
            item.provider === youtube.manifest.id &&
            item.plugin_installation_id === youtube.id &&
            item.provider_recording_id === selection.candidate.id,
        ),
      ).toBe(true);
      const afterPlaylist = await readSpotifyYouTube<Schema["PlaylistDetail"]>(
        page,
        `/api/v1/playlists/${receipt.playlist_id}`,
      );
      expect(
        JSON.stringify(retainedImportedPlaylist(afterPlaylist, recording)) ===
          JSON.stringify(retainedImportedPlaylist(beforePlaylist, recording)),
      ).toBe(true);
      expect(
        afterPlaylist.entries
          .filter((entry) => entry.recording_id === recording)
          .every((entry) => entry.available),
      ).toBe(true);
      expect(
        afterPlaylist.entries
          .filter((entry) => entry.recording_id !== recording)
          .every((entry) => !entry.available),
      ).toBe(true);
      expect(
        JSON.stringify(
          await readSpotifyYouTube<Schema["Playlist"][]>(
            page,
            "/api/v1/playlists",
          ),
        ) === JSON.stringify(beforeLists),
      ).toBe(true);
      expect(
        (
          await readSpotifyYouTube<Schema["RecordingPage"]>(
            page,
            "/api/v1/recordings?limit=1",
          )
        ).total,
      ).toBe(beforeCatalog.total);
      const sources = await readSpotifyYouTube<Schema["Source"][]>(
        page,
        `/api/v1/recordings/${recording}/sources`,
      );
      expect(
        sources.length > 0 &&
          sources.every(
            (source) =>
              uuid.test(source.id) &&
              source.ready &&
              source.recording_id === recording &&
              source.provider === "plugin",
          ),
      ).toBe(true);

      phase = 8;
      await page
        .getByRole("navigation", { name: "Main navigation" })
        .getByRole("link", { name: "Playlists", exact: true })
        .click();
      await page
        .locator(`a[href="/playlists/${receipt.playlist_id}"]`)
        .first()
        .click();
      const playableTable = page.getByRole("table", {
        name: "Playlist entries",
        exact: true,
      });
      await expect(playableTable.getByRole("row")).toHaveCount(
        beforePlaylist.entries.length + 1,
      );
      await expect(playableTable.locator(".row-play:disabled")).toHaveCount(
        afterPlaylist.entries.filter(
          (entry) => entry.recording_id && entry.recording_id !== recording,
        ).length,
      );
      expect(
        await playableTable
          .locator("tbody > tr")
          .evaluateAll((rows) =>
            rows.every((row) => row.getBoundingClientRect().height > 0),
          ),
      ).toBe(true);
      const playableRow = playableTable
        .getByRole("row")
        .filter({ has: page.locator(`a[href="/recordings/${recording}"]`) })
        .first();
      const play = playableRow.getByRole("button", { name: /^Play .+/ });
      await expect(play).toBeEnabled();
      await play.click();
      await expect
        .poll(
          async () => {
            const state = await liveAudioState(page);
            return (
              state.time > 0.3 &&
              state.ready >= 2 &&
              state.error === 0 &&
              !state.paused
            );
          },
          { timeout: 30_000 },
        )
        .toBe(true);
      const first = await liveAudioState(page);
      const source = new URL(first.source);
      expect(
        source.origin === origin &&
          !source.search &&
          !source.hash &&
          audioPath.test(source.pathname) &&
          sources.some(
            (item) => source.pathname === `/api/v1/sources/${item.id}/audio`,
          ),
      ).toBe(true);
      expect(
        Number.isFinite(first.duration) &&
          first.duration > 10 &&
          first.duration <= 600 &&
          Math.abs(first.duration * 1000 - original.duration_ms) <=
            Math.max(10_000, original.duration_ms * 0.08),
      ).toBe(true);
      await expect
        .poll(async () => (await liveAudioState(page)).time, {
          timeout: 10_000,
        })
        .toBeGreaterThan(first.time + 0.2);

      phase = 9;
      const seek = page.getByRole("slider", { name: "Seek", exact: true });
      await expect(seek).toBeEnabled();
      const beforeSeek = await liveAudioState(page);
      await seek.focus();
      await seek.press("PageUp");
      await seek.press("PageUp");
      const target = Number(await seek.inputValue());
      expect(target).toBeGreaterThan(beforeSeek.time + 1);
      await expect
        .poll(
          async () => {
            const state = await liveAudioState(page);
            return (
              state.time >= target - 0.5 &&
              state.ready >= 2 &&
              state.error === 0 &&
              !state.paused &&
              !state.seeking
            );
          },
          { timeout: 20_000 },
        )
        .toBe(true);
      const afterSeek = await liveAudioState(page);
      expect(afterSeek.time < target + 5).toBe(true);

      phase = 10;
      await page
        .getByRole("navigation", { name: "Main navigation" })
        .getByRole("link", { name: "History", exact: true })
        .click();
      await expect
        .poll(async () => (await liveAudioState(page)).time, {
          timeout: 10_000,
        })
        .toBeGreaterThan(afterSeek.time + 0.2);
      const navigated = await liveAudioState(page);
      expect(
        navigated.source === first.source &&
          navigated.ready >= 2 &&
          navigated.error === 0 &&
          !navigated.paused,
      ).toBe(true);
      await liveControlBounds(page, seek, false);
      const preserved = await readSpotifyYouTube<Schema["PlaylistDetail"]>(
        page,
        `/api/v1/playlists/${receipt.playlist_id}`,
      );
      expect(JSON.stringify(preserved) === JSON.stringify(afterPlaylist)).toBe(
        true,
      );
      const history = await readSpotifyYouTube<Schema["Activity"]>(
        page,
        "/api/v1/activity",
      );
      expect(history.history.some((item) => item.id === recording)).toBe(true);
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
