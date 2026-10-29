import { expect, test } from "@playwright/test";
import type { components } from "../src/generated";
import {
  audioPath,
  liveAudio,
  liveConsolePrivacy,
  liveFeed,
  livePassword,
  liveReader,
  watchLiveAudio,
} from "./fixtures/podcast-live";
import { openLibrarySection } from "./fixtures/library-sections";

type Schema = components["schemas"];
test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});
test.skip(
  process.env.HITO_PODCAST_LIVE_E2E !== "1",
  "Requires explicit live-feed opt-in and a fresh packaged disposable server",
);

test("a public RSS episode plays through the packaged server without an implicit download", async ({
  page,
  context,
  baseURL,
}) => {
  test.setTimeout(150_000);
  page.setDefaultTimeout(10_000);
  page.setDefaultNavigationTimeout(15_000);
  let stage = "configuration";
  let failure = false;
  let cleanupFailed = false;
  let requests = 0;
  let audioRequests = 0;
  let downloadWrites = 0;
  let foreignRequests = 0;
  let pageErrors = 0;
  let mediaStatus = 0;
  let expectedSource = "";
  let failureMediaCaptured = false;
  let mediaObservation = {
    source_matches: false,
    playing: false,
    seeking: false,
    media_error: 0,
    time_past_two: false,
    duration_finite: false,
    duration_over_seven: false,
  };
  let navigationObservation = {
    captured: false,
    same: false,
    pause: 0,
    emptied: 0,
    ended: 0,
    error: 0,
  };
  let navigationAudio: Awaited<ReturnType<typeof watchLiveAudio>> | null = null;
  const read = liveReader(page);
  const privacy = await liveConsolePrivacy(context);
  page.on("pageerror", () => pageErrors++);
  try {
    const feed = liveFeed();
    privacy.forbid(feed);
    expect(baseURL === "http://localhost:8080").toBe(true);
    const origin = new URL(baseURL!).origin;
    await context.route("**/*", async (route) => {
      const request = route.request();
      const target = new URL(request.url());
      requests++;
      if (target.origin !== origin) foreignRequests++;
      if (audioPath.test(target.pathname)) audioRequests++;
      if (request.method() === "POST" && /\/download$/.test(target.pathname))
        downloadWrites++;
      if (target.origin !== origin || requests > 400 || audioRequests > 32)
        await route.abort();
      else await route.continue();
    });
    stage = "fresh setup";
    expect(
      (await read<Schema["SetupStatus"]>("/api/v1/setup")).needs_setup,
    ).toBe(true);
    await page.goto("/");
    await page.getByLabel("Username", { exact: true }).fill("admin");
    await page.getByLabel("Password", { exact: true }).fill(livePassword);
    await page
      .getByRole("button", { name: "Create administrator", exact: true })
      .click();
    stage = "setup library heading visibility";
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    const navigation = page.getByRole("navigation", {
      name: "Main navigation",
    });
    stage = "initial podcasts navigation";
    await openLibrarySection(page, "Podcasts");
    expect(
      (await read<Schema["PodcastSubscription"][]>("/api/v1/podcasts")).length,
    ).toBe(0);
    stage = "public feed subscription";
    await page.getByLabel("Feed address", { exact: true }).fill(feed);
    const [response] = await Promise.all([
      page.waitForResponse(
        (candidate) =>
          new URL(candidate.url()).pathname === "/api/v1/podcasts" &&
          candidate.request().method() === "POST",
        { timeout: 15_000 },
      ),
      page.getByRole("button", { name: "Subscribe", exact: true }).click(),
    ]);
    expect(response.status()).toBe(202);
    const receipt: Schema["PodcastSubscriptionReceipt"] = await response.json();
    const submitted: Schema["SubscribePodcast"] = response
      .request()
      .postDataJSON();
    expect(
      receipt.request_id === submitted.request_id &&
        submitted.feed_url === feed,
    ).toBe(true);
    let subscription: Schema["PodcastSubscription"] | undefined;
    await expect
      .poll(
        async () => {
          const subscriptions =
            await read<Schema["PodcastSubscription"][]>("/api/v1/podcasts");
          expect(subscriptions.length).toBe(1);
          subscription = subscriptions.find(
            (item) => item.subscription_id === receipt.subscription_id,
          );
          if (subscription?.refresh_failed)
            throw new Error("The live feed check failed");
          return (
            !!subscription &&
            !subscription.refresh_pending &&
            subscription.last_refreshed_at != null &&
            subscription.content_revision > 0
          );
        },
        { timeout: 60_000, intervals: [1_000] },
      )
      .toBe(true);
    privacy.forbid(subscription?.title);
    const podcast = page.locator(
      `a[href="/podcasts/${receipt.subscription_id}"]`,
    );
    stage = "subscription link visibility";
    await expect(podcast).toBeVisible({ timeout: 10_000 });
    stage = "successful feed status visibility";
    await expect(
      page.getByText("Last checked", { exact: false }),
    ).toBeVisible();
    stage = "pending feed status absence";
    await expect(page.getByText("Checking feed…", { exact: true })).toHaveCount(
      0,
    );
    stage = "imported podcast navigation";
    await podcast.click();
    stage = "imported episode archive read";
    const archive = await read<Schema["PodcastEpisodePage"]>(
      `/api/v1/podcasts/${receipt.subscription_id}/episodes`,
    );
    expect(archive.total > 0 && archive.items.length > 0).toBe(true);
    const episode = archive.items[0];
    const recording = episode.recording.id;
    privacy.forbid(episode.recording.title, episode.recording.artist);
    const sources = await read<Schema["Source"][]>(
      `/api/v1/recordings/${recording}/sources`,
    );
    expect(
      sources.length === 1 &&
        sources[0].provider === "podcast" &&
        sources[0].ready &&
        sources[0].recording_id === recording,
    ).toBe(true);
    const source = sources[0].id;
    expectedSource = source;
    expect(audioPath.test(`/api/v1/sources/${source}/audio`)).toBe(true);
    privacy.forbid(`${origin}/api/v1/sources/${source}/audio`);
    const downloadPath = `/api/v1/recordings/${recording}/download`;
    expect(
      (await page.request.get(downloadPath, { timeout: 10_000 })).status(),
    ).toBe(404);
    expect(audioRequests).toBe(0);
    const row = page
      .getByRole("table", { name: "Episodes", exact: true })
      .locator("tbody tr")
      .first();
    expect(
      await row.locator(`a[href="/recordings/${recording}"]`).count(),
    ).toBe(1);
    stage = "episode Play click";
    const [mediaResponse] = await Promise.all([
      page.waitForResponse(
        (candidate) =>
          new URL(candidate.url()).pathname ===
            `/api/v1/sources/${source}/audio` &&
          candidate.request().method() === "GET" &&
          candidate.status() !== 0,
        { timeout: 30_000 },
      ),
      row
        .getByRole("button", { name: /^Play / })
        .click()
        .then(() => {
          stage = "audio response delivery";
        }),
    ]);
    stage = "audio HTTP status acceptance";
    mediaStatus = mediaResponse.status();
    expect([200, 206].includes(mediaStatus)).toBe(true);
    stage = "audio MIME acceptance";
    expect(
      /^audio\//i.test(mediaResponse.headers()["content-type"] ?? ""),
    ).toBe(true);
    stage = "audio advancement";
    await expect
      .poll(
        async () => {
          const state = await liveAudio(page);
          mediaObservation = {
            source_matches: state.sourceId === source,
            playing: !state.paused,
            seeking: state.seeking,
            media_error: state.error,
            time_past_two: state.time > 2,
            duration_finite: Number.isFinite(state.duration),
            duration_over_seven: state.duration > 7,
          };
          return (
            state.sourceId === source &&
            !state.paused &&
            !state.seeking &&
            state.error === 0 &&
            state.time > 2 &&
            Number.isFinite(state.duration) &&
            state.duration > 7
          );
        },
        { timeout: 30_000 },
      )
      .toBe(true);
    stage = "playback time before navigation";
    const beforeNavigation = (await liveAudio(page)).time;
    stage = "navigation audio observer installation";
    navigationAudio = await watchLiveAudio(page);
    stage = "library navigation";
    await navigation
      .getByRole("link", { name: "Library", exact: true })
      .click();
    stage = "navigation library heading visibility";
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    stage = "library navigation audio continuity";
    expect(await navigationAudio.uninterrupted()).toBe(true);
    stage = "podcasts index navigation";
    await openLibrarySection(page, "Podcasts");
    stage = "podcasts index navigation audio continuity";
    expect(await navigationAudio.uninterrupted()).toBe(true);
    stage = "podcast detail navigation";
    await podcast.click();
    stage = "podcast detail navigation audio continuity";
    expect(await navigationAudio.uninterrupted()).toBe(true);
    stage = "recording details navigation";
    await row.locator(`a[href="/recordings/${recording}"]`).click();
    stage = "recording details label visibility";
    await expect(
      page.getByText("Track details", { exact: true }),
    ).toBeVisible();
    stage = "recording details navigation audio continuity";
    expect(await navigationAudio.uninterrupted()).toBe(true);
    stage = "navigation audio advancement";
    await expect
      .poll(
        async () => {
          const state = await liveAudio(page);
          return (
            state.sourceId === source &&
            !state.paused &&
            state.error === 0 &&
            state.time > beforeNavigation
          );
        },
        { timeout: 10_000 },
      )
      .toBe(true);
    stage = "recording metadata read";
    const metadata = await read<Schema["RecordingMetadata"]>(
      `/api/v1/recordings/${recording}/metadata`,
    );
    stage = "recording metadata identity";
    expect(metadata.recording_id === recording).toBe(true);
    const dock = page.getByRole("contentinfo", {
      name: "Audio player",
      exact: true,
    });
    stage = "player episode title identity";
    expect(
      (await dock.locator(".now-playing strong").innerText()) ===
        episode.recording.title,
    ).toBe(true);
    stage = "pre-pause audio continuity";
    expect(await navigationAudio.uninterrupted()).toBe(true);
    stage = "pause action";
    await dock.getByRole("button", { name: "Pause", exact: true }).click();
    stage = "paused media state";
    await expect.poll(async () => (await liveAudio(page)).paused).toBe(true);
    stage = "pause observer event count";
    await expect
      .poll(async () => (await navigationAudio!.snapshot()).pause)
      .toBe(1);
    stage = "paused audio observer snapshot";
    const paused = await navigationAudio.snapshot();
    stage = "paused audio element continuity";
    expect(
      paused.same && paused.emptied + paused.ended + paused.error === 0,
    ).toBe(true);
    stage = "navigation audio observer disposal";
    await navigationAudio.dispose();
    navigationAudio = null;
    stage = "pause seek and resume";
    const seek = dock.getByRole("slider", { name: "Seek", exact: true });
    await seek.focus();
    await seek.press("Home");
    for (let step = 0; step < 50; step++) await seek.press("ArrowRight");
    expect(Math.abs(Number(await seek.inputValue()) - 5) < 0.2).toBe(true);
    await expect
      .poll(
        async () => {
          const state = await liveAudio(page);
          return (
            state.sourceId === source &&
            state.paused &&
            !state.seeking &&
            state.error === 0 &&
            Math.abs(state.time - 5) < 0.5
          );
        },
        { timeout: 15_000 },
      )
      .toBe(true);
    await dock.getByRole("button", { name: "Play", exact: true }).click();
    await expect
      .poll(async () => (await liveAudio(page)).time, { timeout: 15_000 })
      .toBeGreaterThan(5.5);
    await dock.getByRole("button", { name: "Pause", exact: true }).click();
    stage = "no persistent transfer or console disclosure";
    expect(
      (await page.request.get(downloadPath, { timeout: 10_000 })).status(),
    ).toBe(404);
    const finalSources = await read<Schema["Source"][]>(
      `/api/v1/recordings/${recording}/sources`,
    );
    expect(
      finalSources.length === 1 &&
        finalSources[0].id === source &&
        finalSources[0].provider === "podcast" &&
        finalSources[0].ready,
    ).toBe(true);
    expect(downloadWrites + foreignRequests + pageErrors).toBe(0);
    expect(requests <= 400 && audioRequests > 0 && audioRequests <= 32).toBe(
      true,
    );
    expect(await privacy.clean()).toBe(true);
  } catch {
    failure = true;
    try {
      if (navigationAudio) {
        const snapshot = await navigationAudio.snapshot();
        navigationObservation = {
          captured: true,
          same: snapshot.same,
          pause: snapshot.pause,
          emptied: snapshot.emptied,
          ended: snapshot.ended,
          error: snapshot.error,
        };
      }
    } catch {}
    try {
      const state = await liveAudio(page);
      mediaObservation = {
        source_matches: state.sourceId === expectedSource,
        playing: !state.paused,
        seeking: state.seeking,
        media_error: state.error,
        time_past_two: state.time > 2,
        duration_finite: Number.isFinite(state.duration),
        duration_over_seven: state.duration > 7,
      };
      failureMediaCaptured = true;
    } catch {}
  } finally {
    try {
      await navigationAudio?.dispose();
    } catch {
      cleanupFailed = true;
    }
    try {
      await page.evaluate(() => {
        const audio = document.querySelector<HTMLAudioElement>(
          "audio[data-hito-player]",
        );
        if (audio) {
          audio.pause();
          audio.removeAttribute("src");
          audio.load();
        }
      });
      await page.goto("about:blank", { timeout: 5_000 });
    } catch {
      cleanupFailed = true;
    }
  }
  if (failure || cleanupFailed)
    throw new Error(
      `Live RSS verification failed at ${stage}; media_status=${mediaStatus}; failure_media_captured=${failureMediaCaptured}; media_observation=${JSON.stringify(mediaObservation)}; navigation_observation=${JSON.stringify(navigationObservation)}; upstream, UI or media assertion failed`,
    );
});
