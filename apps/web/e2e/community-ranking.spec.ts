import { expect, test, type Request } from "@playwright/test";
import { readFile, stat, writeFile } from "node:fs/promises";
import { isDeepStrictEqual } from "node:util";
import { captureConsolePrivacy } from "./fixtures/console-privacy";
import { downloadAbsence } from "./fixtures/collection-artwork-actions";
import { rankingProviders } from "./fixtures/community-ranking-providers";
import { rankingService } from "./fixtures/community-ranking-service";
import { createNegativeEvidence } from "./fixtures/community-ranking-negative-evidence";
import {
  acknowledgeOwnedProgress,
  assertHeardProgress,
  observeOwnedProgress,
  pauseOwnedPlayback,
  type OwnedPlaybackEpisode,
  type OwnedProgressWrite,
} from "./fixtures/community-ranking-progress";
import {
  exchange,
  prepareRankingLibrary,
} from "./fixtures/community-ranking-journey";
import type { components } from "../src/generated";

type Schema = components["schemas"];
const artworkResponseCeiling = 3 * 4 + 2 * 1;
type Checkpoint =
  | "guard"
  | "play.navigate"
  | "play.click"
  | "play.ready"
  | "play.source_requests"
  | "play.pause"
  | "play.paused"
  | "unsaved.navigate"
  | "unsaved.initial"
  | "unsaved.check"
  | "unsaved.before_play"
  | "unsaved.after_play"
  | "privacy.console";
function diagnosticRoute(value: string) {
  try {
    const path = new URL(value).pathname;
    if (/^\/api\/v1\/recordings\/[^/]+\/artwork$/.test(path))
      return "recording_artwork";
    if (/^\/api\/v1\/recordings\/[^/]+\/download$/.test(path))
      return "recording_download";
    if (/^\/api\/v1\/playlists\/[^/]+\/download$/.test(path))
      return "playlist_download";
    if (/^\/api\/v1\/recordings\/[^/]+\/source-choice$/.test(path))
      return "source_choice";
    if (path === "/favicon.ico") return "favicon";
    if (path === "/api/v1/me") return "account_check";
    return "other";
  } catch {
    return "empty";
  }
}
test.use({ trace: "off", screenshot: "off", video: "off" });

test("a separately saved community ranking preference changes ordinary browser Play without sharing listening", async ({
  page,
  context,
  browser,
  baseURL,
}) => {
  let phase = "isolation";
  let checkpoint: Checkpoint = "guard";
  let failure: string | null = null;
  let failureCause: "invalid_url" | "deadline" | "other" | null = null;
  let providers: Awaited<ReturnType<typeof rankingProviders>> | undefined;
  let community: Awaited<ReturnType<typeof rankingService>> | undefined;
  let library: Awaited<ReturnType<typeof prepareRankingLibrary>> | undefined;
  const privateValues: string[] = [
    "private-ranking-original.wav",
    "/owned-music",
  ];
  const pending = new Set<Promise<void>>();
  const counts = {
    errors: 0,
    external: 0,
    privateLog: 0,
    callbacks: 0,
    cleanup: 0,
    forbiddenWrites: 0,
    rankingWrites: 0,
    decoded: 0,
    consoleErrors: 0,
    networkDiagnostics: 0,
    progressAcks: 0,
  };
  const rankedResponses: unknown[] = [];
  const http: { request_id: string; status: number; route: string }[] = [];
  const failedResponses: { url: string; status: number }[] = [];
  const consoleErrors: { text: string; url: string }[] = [];
  const unexpectedResponses: { status: number; route: string; get: boolean }[] =
    [];
  const negativeEvidence = createNegativeEvidence(context);
  const expectedNegatives = negativeEvidence.counts;
  const negativeChecks = negativeEvidence.checks;
  let logoutCheck: Request | undefined;
  const browserAudio: string[] = [];
  const progressWrites = new Map<Request, OwnedProgressWrite>();
  const progressEpisodes: OwnedPlaybackEpisode[] = [];
  let progressEpisode: OwnedPlaybackEpisode | undefined;
  const mediaSamples: unknown[] = [];
  let savedRequest: unknown;
  const downloads: {
    before?: Awaited<ReturnType<typeof downloadAbsence>>;
    after?: Awaited<ReturnType<typeof downloadAbsence>>;
  } = {};
  async function sampleMedia(source?: string, requests?: number) {
    if (mediaSamples.length >= 12) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      const sample = await Promise.race([
        page.locator("audio[data-hito-player]").evaluate(
          (audio: HTMLAudioElement, expected) => ({
            matches:
              expected === null
                ? null
                : new URL(audio.currentSrc || audio.src).pathname === expected,
            current_ms: Math.floor(audio.currentTime * 1000),
            duration_ms: Math.floor(audio.duration * 1000),
            ready: audio.readyState,
            paused: audio.paused,
            seeking: audio.seeking,
            ended: audio.ended,
            rate: audio.playbackRate,
            played_ms: Array.from(
              { length: Math.min(audio.played.length, 2) },
              (_, i) => [
                Math.floor(audio.played.start(i) * 1000),
                Math.floor(audio.played.end(i) * 1000),
              ],
            ),
          }),
          source ? `/api/v1/sources/${source}/audio` : null,
        ),
        new Promise<never>((_, reject) => {
          timer = setTimeout(
            () => reject(new Error("Owned media diagnostic deadline")),
            2000,
          );
        }),
      ]);
      mediaSamples.push({ phase, checkpoint, requests, ...sample });
    } catch {
      mediaSamples.push({ phase, checkpoint, requests, unavailable: true });
    } finally {
      clearTimeout(timer);
    }
  }
  async function settle() {
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      await Promise.race([
        Promise.all(pending),
        new Promise<never>((_, reject) => {
          timer = setTimeout(
            () => reject(new Error("Owned callbacks exceeded their deadline")),
            5000,
          );
        }),
      ]);
    } finally {
      clearTimeout(timer);
    }
  }
  try {
    if (
      baseURL !== "http://localhost:8080" ||
      process.env.HITO_COMMUNITY_RANKING_E2E !== "1" ||
      process.env.PLAYWRIGHT_NO_COPY_PROMPT !== "1"
    )
      throw new Error("The disposable ranking browser guard is required");
    const original = await readFile(
      "/owned-music/private-ranking-original.wav",
    );
    const originalMetadata = await stat(
      "/owned-music/private-ranking-original.wav",
      { bigint: true },
    );
    providers = await rankingProviders();
    community = await rankingService();
    privateValues.push(...providers.privateValues, ...community.privateValues);
    await captureConsolePrivacy(
      context,
      (text) => {
        if (
          privateValues.some(
            (value) =>
              value.length > 0 &&
              (text.includes(value) ||
                text.includes(JSON.stringify(value).slice(1, -1))),
          )
        )
          counts.privateLog++;
      },
      () => counts.callbacks++,
      pending,
    );
    page.on("pageerror", () => counts.errors++);
    context.on("console", (message) => {
      if (message.type() !== "error") return;
      try {
        if (
          consoleErrors.length >= 256 ||
          Buffer.byteLength(message.text()) > 4096
        )
          throw new Error("Owned console evidence exceeded its bound");
        consoleErrors.push({
          text: message.text(),
          url: message.location().url,
        });
      } catch {
        counts.callbacks++;
      }
    });
    context.on("request", (request) => {
      try {
        const url = new URL(request.url());
        if (url.protocol.startsWith("http") && url.origin !== baseURL)
          counts.external++;
        if (url.origin !== baseURL) return;
        if (
          phase === "logout" &&
          url.pathname === "/api/v1/me" &&
          request.method() === "GET"
        ) {
          if (logoutCheck) counts.callbacks++;
          else logoutCheck = request;
        }
        if (/^\/api\/v1\/sources\/[0-9a-f-]+\/audio$/.test(url.pathname))
          browserAudio.push(url.pathname);
        if (["POST", "PUT", "DELETE"].includes(request.method())) {
          const allowed =
            (request.method() === "PUT" &&
              url.pathname === "/api/v1/community/ranking-preferences") ||
            (request.method() === "POST" &&
              ["/api/v1/sync", "/api/v1/logout"].includes(url.pathname));
          if (!allowed) counts.forbiddenWrites++;
          if (url.pathname === "/api/v1/sync") {
            const write = observeOwnedProgress(page, request, progressEpisode);
            progressWrites.set(request, write);
            const task = write.observed
              .then(() => {})
              .catch(() => {
                counts.callbacks++;
              })
              .finally(() => pending.delete(task));
            pending.add(task);
          }
          if (url.pathname === "/api/v1/community/ranking-preferences") {
            counts.rankingWrites++;
            const raw = request.postData();
            if (!raw || Buffer.byteLength(raw) > 256)
              throw new Error("Owned ranking save exceeded its bound");
            savedRequest = JSON.parse(raw);
          }
        }
      } catch {
        counts.callbacks++;
      }
    });
    context.on("response", (response) => {
      const url = new URL(response.url());
      const path = url.pathname;
      if (response.status() >= 400) {
        const artwork =
          response.status() === 404 &&
          response.request().method() === "GET" &&
          url.origin === baseURL &&
          !url.search &&
          !url.hash &&
          library !== undefined &&
          path === `/api/v1/recordings/${library.recording.id}/artwork` &&
          expectedNegatives.artwork404 < artworkResponseCeiling;
        const logout =
          response.status() === 401 &&
          phase === "logout" &&
          response.request() === logoutCheck &&
          path === "/api/v1/me" &&
          expectedNegatives.logout401 < 1;
        if (artwork || logout) {
          const task = negativeEvidence
            .read(
              response,
              artwork ? "/api/v1/recordings/{id}/artwork" : "/api/v1/me",
            )
            .then(({ url, request_id, status, route }) => {
              failedResponses.push({ url, status });
              http.push({ request_id, status, route });
            })
            .catch(() => {
              counts.callbacks++;
            })
            .finally(() => pending.delete(task));
          pending.add(task);
        } else if (
          response.status() === 404 &&
          response.request().method() === "GET" &&
          /^\/api\/v1\/(recordings|playlists)\/[^/]+\/(download|source-choice)$/.test(
            path,
          ) &&
          failedResponses.length < 256
        )
          failedResponses.push({
            url: response.url(),
            status: response.status(),
          });
        else {
          counts.callbacks++;
          if (unexpectedResponses.length < 32)
            unexpectedResponses.push({
              status: response.status(),
              route: diagnosticRoute(response.url()),
              get: response.request().method() === "GET",
            });
        }
      }
      const playback = path.endsWith("/playback-sources");
      const progress = path === "/api/v1/sync";
      const rankingPreference =
        path === "/api/v1/community/ranking-preferences";
      if (!playback && !rankingPreference && !progress) return;
      const task = (async () => {
        if (progress) {
          http.push(
            await acknowledgeOwnedProgress(
              response,
              progressWrites.get(response.request()),
            ),
          );
          return;
        }
        const headers = await response.allHeaders();
        if (
          response.status() !== 200 ||
          !headers["cache-control"]?.includes("no-store") ||
          !/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(
            headers["x-request-id"] ?? "",
          ) ||
          http.length >= 32
        )
          throw new Error("Owned ranking HTTP evidence differs");
        http.push({
          request_id: headers["x-request-id"],
          status: response.status(),
          route: playback
            ? "/api/v1/recordings/{id}/playback-sources"
            : "/api/v1/community/ranking-preferences",
        });
        if (playback) {
          const bytes = await response.body();
          if (bytes.length > 8192)
            throw new Error("Owned source response exceeded its bound");
          rankedResponses.push(JSON.parse(bytes.toString("utf8")));
        }
      })()
        .catch(() => {
          counts.callbacks++;
        })
        .finally(() => pending.delete(task));
      pending.add(task);
    });
    phase = "public_adoption_and_matching";
    library = await prepareRankingLibrary(page, browser, baseURL, providers);
    privateValues.push(...library.privateValues);
    const preferredIndex = providers.providers.findIndex(
      (provider) => provider.label === library!.baseline[1].label,
    );
    expect(preferredIndex >= 0).toBe(true);
    await community.seed(library.refs, library.refs[1]);
    privateValues.push(...community.privateValues);
    const providerBaseline = providers.snapshot();
    const identity = await library.identity();
    const peer = await library.peerState();
    const oldFlags = await exchange<Schema["CommunityPreferences"]>(
      page,
      "/community/preferences",
      library.account,
    );
    expect(
      isDeepStrictEqual(oldFlags, {
        revision: 0,
        playback_reporting: false,
        metadata_corrections: false,
      }),
    ).toBe(true);
    expect(
      isDeepStrictEqual(
        await exchange<Schema["CommunityRankingPreferences"]>(
          page,
          "/community/ranking-preferences",
          library.account,
        ),
        { revision: 0, use_source_rankings: false },
      ),
    ).toBe(true);
    downloads.before = await downloadAbsence(
      page,
      library.account,
      [library.recording.id],
      [library.playlist],
    );
    const sourcePath = (id: string) => `/api/v1/sources/${id}/audio`;
    async function play(expectedSource: string) {
      checkpoint = "play.navigate";
      await page.goto(`/playlists/${library!.playlist}`);
      const episode: OwnedPlaybackEpisode = {
        recording: library!.recording.id,
        source: expectedSource,
        writes: [],
      };
      progressEpisode = episode;
      progressEpisodes.push(episode);
      const before = browserAudio.length;
      checkpoint = "play.click";
      await page
        .getByRole("button", { name: "Play playlist", exact: true })
        .click();
      const audio = page.locator("audio[data-hito-player]");
      checkpoint = "play.ready";
      await expect
        .poll(
          () =>
            audio.evaluate(
              (element: HTMLAudioElement, path) =>
                Boolean(element.currentSrc || element.src) &&
                new URL(element.currentSrc || element.src).pathname === path &&
                element.currentTime > 0.3 &&
                !element.paused &&
                element.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA &&
                element.error === null,
              sourcePath(expectedSource),
            ),
          { timeout: 10_000 },
        )
        .toBe(true);
      await sampleMedia(expectedSource, browserAudio.length - before);
      checkpoint = "play.source_requests";
      expect(
        browserAudio.some((path) => path === sourcePath(expectedSource)) &&
          browserAudio
            .slice(before)
            .every((path) => path === sourcePath(expectedSource)),
      ).toBe(true);
      checkpoint = "play.pause";
      await pauseOwnedPlayback(page, episode);
      counts.progressAcks++;
      checkpoint = "play.paused";
      await expect
        .poll(() =>
          audio.evaluate((element: HTMLAudioElement) => element.paused),
        )
        .toBe(true);
      await sampleMedia(expectedSource, browserAudio.length - before);
    }
    phase = "default_disabled_playback";
    await play(library.baseline[0].id);
    expect(
      community.counters.rankings === 0 && counts.rankingWrites === 0,
    ).toBe(true);
    phase = "unsaved_choice";
    checkpoint = "unsaved.navigate";
    await page.goto("/settings?section=privacy");
    const ranking = page.getByRole("checkbox", {
      name: "Use community source rankings",
      exact: true,
    });
    checkpoint = "unsaved.initial";
    await expect(ranking).not.toBeChecked();
    checkpoint = "unsaved.check";
    await ranking.check();
    checkpoint = "unsaved.before_play";
    expect(
      counts.rankingWrites === 0 && community.counters.rankings === 0,
    ).toBe(true);
    await play(library.baseline[0].id);
    checkpoint = "unsaved.after_play";
    expect(
      community.counters.rankings === 0 && counts.rankingWrites === 0,
    ).toBe(true);
    phase = "explicit_separate_save";
    await page.goto("/settings?section=privacy");
    await expect(ranking).not.toBeChecked();
    await ranking.check();
    await page
      .getByRole("button", {
        name: "Save source ranking preference",
        exact: true,
      })
      .click();
    await expect(
      page.getByText("Source ranking preference saved.", { exact: true }),
    ).toBeVisible();
    expect(
      counts.rankingWrites === 1 &&
        isDeepStrictEqual(savedRequest, {
          revision: 0,
          use_source_rankings: true,
        }) &&
        community.counters.rankings === 0,
    ).toBe(true);
    expect(
      isDeepStrictEqual(
        await exchange<Schema["CommunityPreferences"]>(
          page,
          "/community/preferences",
          library.account,
        ),
        oldFlags,
      ),
    ).toBe(true);
    phase = "ranked_actual_playback";
    await play(library.baseline[1].id);
    await settle();
    expect(
      community.counters.rankings === 1 &&
        community.counters.completed === 1 &&
        community.counters.invalid === 0 &&
        rankedResponses.length === 3 &&
        isDeepStrictEqual(rankedResponses, [
          library.baseline,
          library.baseline,
          [...library.baseline].reverse(),
        ]),
    ).toBe(true);
    phase = "owned_pcm_decode";
    const bytesResponse = await page.request.get(
      sourcePath(library.baseline[1].id),
      { timeout: 5000, maxRedirects: 0 },
    );
    expect(
      bytesResponse.status() === 200 &&
        Number(bytesResponse.headers()["content-length"]) ===
          providers.providers[preferredIndex].wav.length,
    ).toBe(true);
    const bytes = await bytesResponse.body();
    expect(bytes.equals(providers.providers[preferredIndex].wav)).toBe(true);
    const pcm = await page.evaluate(
      async (encoded) => {
        const audio = new AudioContext({ sampleRate: 16_000 });
        try {
          const document = await audio.decodeAudioData(
            Uint8Array.from(encoded).buffer,
          );
          return {
            channels: document.numberOfChannels,
            rate: document.sampleRate,
            length: document.length,
            duration: document.duration,
            sample: document.getChannelData(0)[100],
          };
        } finally {
          await audio.close();
        }
      },
      [...bytes],
    );
    expect(
      pcm.channels === 1 &&
        pcm.rate === 16_000 &&
        pcm.length === 192_000 &&
        pcm.duration === 12 &&
        Math.abs(
          pcm.sample -
            providers.providers[preferredIndex].wav.readInt16LE(244) / 32768,
        ) < 0.0001,
    ).toBe(true);
    counts.decoded++;
    phase = "preserved_state";
    expect(
      (await library.identity()) === identity &&
        (await library.peerState()) === peer,
    ).toBe(true);
    expect(
      providers
        .snapshot()
        .every(
          (provider, index) =>
            ["manifest", "search", "detail", "sources"].every(
              (key) =>
                provider[key as keyof typeof provider] ===
                providerBaseline[index][key as keyof typeof provider],
            ) &&
            provider.audioGet > 0 &&
            provider.audio <= 16 &&
            provider.errors === 0,
        ),
    ).toBe(true);
    downloads.after = await downloadAbsence(
      page,
      library.account,
      [library.recording.id],
      [library.playlist],
    );
    expect(
      (await readFile("/owned-music/private-ranking-original.wav")).equals(
        original,
      ),
    ).toBe(true);
    const after = await stat("/owned-music/private-ranking-original.wav", {
      bigint: true,
    });
    expect(
      after.size === originalMetadata.size &&
        after.mtimeNs === originalMetadata.mtimeNs &&
        after.ino === originalMetadata.ino,
    ).toBe(true);
    phase = "logout";
    await page.getByRole("button", { name: "Sign out", exact: true }).click();
    await expect(
      page.getByRole("button", { name: "Sign in", exact: true }),
    ).toBeVisible();
    expect(await page.locator("audio[data-hito-player]").count()).toBe(0);
    await exchange(
      page,
      "/community/ranking-preferences",
      undefined,
      "GET",
      undefined,
      401,
    );
    await exchange(
      page,
      `/recordings/${library.recording.id}/playback-sources`,
      undefined,
      "GET",
      undefined,
      401,
    );
    expect(
      community.counters.rankings === 1 && counts.forbiddenWrites === 0,
    ).toBe(true);
    expect(progressWrites.size === 3 && progressEpisodes.length === 3).toBe(
      true,
    );
    for (const episode of progressEpisodes) {
      expect(episode.writes.length === 1 && !!episode.afterPause).toBe(true);
      const write = episode.writes[0];
      expect(!!write.receipt).toBe(true);
      assertHeardProgress(write.batch, episode.recording, episode.afterPause!);
    }
  } catch (error) {
    failure = phase;
    const message = error instanceof Error ? error.message : "";
    failureCause = message.includes("Invalid URL")
      ? "invalid_url"
      : /[Tt]imeout|deadline/.test(message)
        ? "deadline"
        : "other";
    await sampleMedia(progressEpisode?.source);
  } finally {
    try {
      await settle();
    } catch {
      counts.cleanup++;
    }
    try {
      await library?.peerContext.close();
    } catch {
      counts.cleanup++;
    }
    try {
      negativeEvidence.close();
      await context.close();
    } catch {
      counts.cleanup++;
    }
    try {
      await settle();
    } catch {
      counts.cleanup++;
    }
    try {
      await community?.close();
    } catch {
      counts.cleanup++;
    }
    try {
      await providers?.close();
    } catch {
      counts.cleanup++;
    }
    const correlated = new Set<number>();
    if (!failure) checkpoint = "privacy.console";
    for (const diagnostic of consoleErrors) {
      const index = failedResponses.findIndex(
        (response, index) =>
          !correlated.has(index) &&
          response.url === diagnostic.url &&
          diagnostic.text ===
            (response.status === 401
              ? "Failed to load resource: the server responded with a status of 401 (Unauthorized)"
              : "Failed to load resource: the server responded with a status of 404 (Not Found)"),
      );
      if (index < 0) counts.consoleErrors++;
      else {
        correlated.add(index);
        counts.networkDiagnostics++;
      }
    }
    if (correlated.size !== failedResponses.length) counts.callbacks++;
    if (
      pending.size ||
      counts.errors ||
      counts.consoleErrors ||
      counts.external ||
      counts.privateLog ||
      counts.callbacks ||
      counts.cleanup ||
      providers?.providers.some((provider) => provider.counts.errors)
    )
      failure ??= "privacy_or_cleanup";
  }
  if (failure || !providers || !community) {
    await writeFile(
      "/evidence/failure-proof.json",
      JSON.stringify({
        phase: failure,
        cause: failureCause,
        checkpoint,
        counts,
        expected_negatives: expectedNegatives,
        negative_checks: negativeChecks,
        unexpected_responses: unexpectedResponses,
        native_errors: consoleErrors.map((diagnostic) => ({
          route: diagnosticRoute(diagnostic.url),
          resource404:
            diagnostic.text ===
            "Failed to load resource: the server responded with a status of 404 (Not Found)",
          resource401:
            diagnostic.text ===
            "Failed to load resource: the server responded with a status of 401 (Unauthorized)",
        })),
        expected_error_responses: failedResponses.map((response) => ({
          status: response.status,
          route: diagnosticRoute(response.url),
        })),
        media: mediaSamples,
        progress: progressEpisodes.map((episode) => ({
          before_ms: episode.beforePause?.position_ms ?? null,
          after_ms: episode.afterPause?.position_ms ?? null,
          prefix_ms: episode.afterPause?.prefix_ms ?? null,
          writes: episode.writes.length,
          acknowledgements: episode.writes.filter((write) => write.receipt)
            .length,
          positions_ms: episode.writes.flatMap((write) =>
            write.batch.operations.map(({ action }) =>
              action.kind === "progress_set" ? action.position_ms : -1,
            ),
          ),
        })),
      }) + "\n",
      { flag: "wx", mode: 0o600 },
    );
    const error = new Error(
      `Owned community ranking browser gate failed at ${failure ?? "missing_owned_fixture"}`,
    );
    error.stack = error.message;
    throw error;
  }
  await writeFile(
    "/evidence/private-canaries.json",
    JSON.stringify(privateValues),
    { flag: "wx", mode: 0o600 },
  );
  await writeFile(
    "/evidence/browser-proof.json",
    JSON.stringify({
      counts,
      expected_negatives: expectedNegatives,
      negative_checks: negativeChecks,
      community: community.counters,
      http,
      downloads,
      ranking_request_ids: community.requestIds,
      seeded_receipts: community.receipts,
      providers: providers.snapshot(),
      progress: progressEpisodes.map((episode) => ({
        before_ms: episode.beforePause!.position_ms,
        after_ms: episode.afterPause!.position_ms,
        prefix_ms: episode.afterPause!.prefix_ms,
        writes: episode.writes.map((write) => ({
          request_id: write.receipt!.request_id,
          operation_ids: write.batch.operations.map(({ id }) => id),
          positions_ms: write.batch.operations.map(({ action }) =>
            action.kind === "progress_set" ? action.position_ms : -1,
          ),
        })),
      })),
    }) + "\n",
    { flag: "wx", mode: 0o600 },
  );
});
