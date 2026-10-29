import { expect, test, type Page, type Response } from "@playwright/test";
import type { components } from "../src/generated";
import { openLibrarySection } from "./fixtures/library-sections";

type Schema = components["schemas"];
type FixtureStatus = {
  kind: string;
  feed_url: string;
  feed_requests: number;
  enclosure_requests: number;
  unexpected_requests: number;
};

const title = "Owned playable episode";
const feedTitle = "Owned audio feed";
const origin = "http://localhost:8080";
const audioPath = /^\/api\/v1\/sources\/[a-f0-9-]+\/audio$/;

test.use({ launchOptions: { args: [] } });

test.skip(
  process.env.HITO_PODCAST_E2E !== "1",
  "Requires the disposable podcast_browser example host",
);

test("a subscribed RSS episode plays, survives navigation and resumes saved progress", async ({
  page,
  context,
  baseURL,
  browserName,
}) => {
  expect(baseURL).toBe(origin);
  const setup = await page.request.get("/api/v1/setup");
  expect(setup.status()).toBe(200);
  expect(await setup.json()).toEqual({ needs_setup: true });
  const owned = await fixtureStatus(page);
  expect(owned.kind).toBe("hito-owned-podcast-browser");
  expect(new URL(owned.feed_url).origin).toMatch(/^http:\/\/127\.0\.0\.1:\d+$/);
  expect(new URL(owned.feed_url).pathname).toBe("/feed.xml");
  expect(owned.feed_requests).toBe(0);
  expect(owned.enclosure_requests).toBe(0);

  const errors: string[] = [];
  const foreignOrigins: string[] = [];
  const audioRequests: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (request) => {
    const url = new URL(request.url());
    if (audioPath.test(url.pathname)) audioRequests.push(url.pathname);
  });
  await context.route("**/*", async (route) => {
    const requestedOrigin = new URL(route.request().url()).origin;
    if (requestedOrigin !== origin) {
      foreignOrigins.push(requestedOrigin);
      await route.abort();
    } else {
      await route.continue();
    }
  });

  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "Set up Hito" }),
  ).toBeVisible();
  await page.getByLabel("Username", { exact: true }).fill("admin");
  await page
    .getByLabel("Password", { exact: true })
    .fill("owned podcast browser password");
  await page
    .getByRole("button", { name: "Create administrator", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
  const navigation = page.getByRole("navigation", { name: "Main navigation" });
  await openLibrarySection(page, "Podcasts");
  await expect(
    page.getByRole("heading", { name: "Podcasts", exact: true }),
  ).toBeVisible();
  await page.getByLabel("Feed address", { exact: true }).fill(owned.feed_url);
  const subscribed = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname === "/api/v1/podcasts" &&
      response.request().method() === "POST",
  );
  await page.getByRole("button", { name: "Subscribe", exact: true }).click();
  expect((await subscribed).status()).toBe(202);
  const podcast = page.getByRole("link", { name: feedTitle, exact: true });
  await expect(podcast).toBeVisible({ timeout: 15_000 });
  await expect(page.getByText("Last checked", { exact: false })).toBeVisible();
  await expect(page.getByText("Checking feed…", { exact: true })).toHaveCount(
    0,
  );
  const podcastPath = await podcast.getAttribute("href");
  if (!podcastPath)
    throw new Error("The podcast detail link is missing its address");
  const detailUrl = new URL(podcastPath, origin).href;
  await podcast.click();
  await expect(page).toHaveURL(detailUrl);
  await expect(
    page.getByRole("heading", { name: feedTitle, exact: true }),
  ).toBeVisible();
  const episodes = page.getByRole("table", { name: "Episodes", exact: true });
  await expect(episodes.getByRole("row")).toHaveCount(2);
  const playEpisode = episodes.getByRole("button", {
    name: `Play ${title}`,
    exact: true,
  });
  await expect(playEpisode).toBeEnabled();
  await expect(episodes.getByRole("button", { name: /Download/ })).toHaveCount(
    0,
  );
  const subscription = new URL(detailUrl).pathname.split("/").at(-1)!;
  const published = await page.request.get(
    `/api/v1/podcasts/${subscription}/episodes`,
  );
  expect(published.status()).toBe(200);
  const archive: Schema["PodcastEpisodePage"] = await published.json();
  expect(archive.total).toBe(1);
  const recording = archive.items[0].recording.id;
  await expectNoDownload(page, recording);
  const beforePlay = await fixtureStatus(page);
  expect(beforePlay.feed_requests).toBe(1);
  expect(beforePlay.enclosure_requests).toBe(0);
  expect(beforePlay.unexpected_requests).toBe(0);
  expect(audioRequests).toEqual([]);
  const audio = page.locator("audio[data-hito-player]");
  await expect(audio).toHaveCount(1);
  expect(
    await audio.evaluate((element: HTMLAudioElement) => ({
      paused: element.paused,
      position: element.currentTime,
      source: element.currentSrc,
    })),
  ).toEqual({ paused: true, position: 0, source: "" });

  const delivered = page.waitForResponse(isAudio);
  await playEpisode.click();
  const response = await delivered;
  expect(response.request().resourceType()).toBe(
    browserName === "webkit" ? "other" : "media",
  );
  expect(response.headers()["content-type"]).toBe("audio/wav");
  expect(response.headers()["accept-ranges"]).toBe("bytes");
  const bytes = await response.body();
  expect(bytes.length).toBe(720_044);
  expect(bytes.subarray(0, 4).toString()).toBe("RIFF");
  const position = () =>
    audio.evaluate((element: HTMLAudioElement) => element.currentTime);
  await expect.poll(position).toBeGreaterThan(0.2);
  expect(
    await audio.evaluate((element: HTMLAudioElement) => element.duration),
  ).toBe(45);
  const source = await audio.evaluate(
    (element: HTMLAudioElement) => element.currentSrc,
  );
  expect(source).toBe(response.url());
  expect(new URL(source).origin).toBe(origin);
  expect(new URL(source).pathname).toMatch(audioPath);
  expect(source).not.toContain("episode.wav");
  const dock = page.getByRole("contentinfo", {
    name: "Audio player",
    exact: true,
  });
  await expect(dock.locator(".now-playing strong")).toHaveText(title);
  const beforeNavigation = await position();
  await navigation
    .getByRole("link", { name: "Biblioteca", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
  await openLibrarySection(page, "Podcasts");
  await page.getByRole("link", { name: feedTitle, exact: true }).click();
  await expect(page).toHaveURL(detailUrl);
  await expect.poll(position).toBeGreaterThan(beforeNavigation);
  expect(
    await audio.evaluate((element: HTMLAudioElement) => element.currentSrc),
  ).toBe(source);

  const saved = page.waitForResponse((candidate) =>
    savedProgress(candidate, recording, 10_000),
  );
  const seek = dock.getByRole("slider", { name: "Seek", exact: true });
  await seek.focus();
  await seek.press("Home");
  await seek.press("PageUp");
  await seek.press("PageUp");
  await seek.press("PageUp");
  await expect.poll(position).toBeGreaterThan(10);
  await dock.getByRole("button", { name: "Pause", exact: true }).click();
  expect((await saved).status()).toBe(200);
  expect(
    await audio.evaluate((element: HTMLAudioElement) => element.paused),
  ).toBe(true);
  const pausedMs = Math.floor((await position()) * 1000);
  expect(pausedMs).toBeLessThan(30_000);
  let durableMs = 0;
  await expect
    .poll(async () => {
      const progress = await page.request.post("/api/v1/recordings/progress", {
        headers: { origin, "x-hito-request": "1" },
        data: {
          recording_ids: [recording],
        } satisfies Schema["SavedPlaybackRequest"],
      });
      expect(progress.status()).toBe(200);
      const stored: Schema["RecordingProgress"][] = await progress.json();
      expect(stored).toHaveLength(1);
      durableMs = stored[0].position_ms;
      return Math.abs(durableMs - pausedMs);
    })
    .toBeLessThan(250);

  await page.reload();
  const resume = episodes.getByRole("button", {
    name: `Resume ${title}`,
    exact: true,
  });
  await expect(resume).toBeVisible();
  expect(await position()).toBe(0);
  expect(
    await audio.evaluate((element: HTMLAudioElement) => element.paused),
  ).toBe(true);
  await resume.click();
  await expect.poll(position).toBeGreaterThan(durableMs / 1000 - 0.1);
  expect(await position()).toBeLessThan(durableMs / 1000 + 3);
  await expect(
    dock.getByRole("button", { name: "Pause", exact: true }),
  ).toBeEnabled();
  await episodes
    .getByRole("button", { name: `Start over ${title}`, exact: true })
    .click();
  await expect.poll(position).toBeLessThan(1);
  await expect.poll(position).toBeGreaterThan(0.2);
  expect(await position()).toBeLessThan(2);
  await dock.getByRole("button", { name: "Pause", exact: true }).click();

  const restartedMs = Math.floor((await position()) * 1000);
  await expect
    .poll(async () =>
      Math.abs(
        (await recordingProgress(page, recording)).position_ms - restartedMs,
      ),
    )
    .toBeLessThan(250);
  await episodes
    .getByRole("button", { name: `Like ${title}`, exact: true })
    .click();
  const unlike = episodes.getByRole("button", {
    name: `Unlike ${title}`,
    exact: true,
  });
  await expect(unlike).toBeVisible();
  const retainedProgress = await recordingProgress(page, recording);
  const beforeRefresh = await fixtureStatus(page);
  const requestsBeforeRefresh = audioRequests.length;
  const mediaBeforeRefresh = await audio.evaluate(
    (element: HTMLAudioElement) => ({
      paused: element.paused,
      position: element.currentTime,
      source: element.currentSrc,
    }),
  );
  expect(mediaBeforeRefresh.paused).toBe(true);
  expect(beforeRefresh.feed_requests).toBe(1);
  const refreshPath = `/api/v1/podcasts/${subscription}/refresh`;
  const refreshAccepted = page.waitForResponse(
    (candidate) =>
      new URL(candidate.url()).pathname === refreshPath &&
      candidate.request().method() === "POST",
  );
  const refreshedEpisodes = page.waitForResponse(
    (candidate) =>
      new URL(candidate.url()).pathname ===
        `/api/v1/podcasts/${subscription}/episodes` &&
      candidate.request().method() === "GET" &&
      candidate.status() === 200,
  );
  const refreshedProgress = page.waitForResponse(
    (candidate) =>
      new URL(candidate.url()).pathname === "/api/v1/recordings/progress" &&
      candidate.request().method() === "POST" &&
      candidate.status() === 200 &&
      (
        candidate.request().postDataJSON() as Schema["SavedPlaybackRequest"]
      ).recording_ids.includes(recording),
  );
  const refresh = page.getByRole("button", {
    name: "Refresh feed",
    exact: true,
  });
  await refresh.click();
  const accepted = await refreshAccepted;
  expect(accepted.status()).toBe(202);
  const requested: Schema["RefreshPodcast"] = accepted.request().postDataJSON();
  const receipt: Schema["PodcastRefreshReceipt"] = await accepted.json();
  expect(receipt.request_id).toBe(requested.request_id);
  expect(receipt.subscription_id).toBe(subscription);
  expect(receipt.job_id).toMatch(/^[a-f0-9-]{36}$/);
  await expect
    .poll(async () => (await fixtureStatus(page)).feed_requests)
    .toBe(2);
  await expect
    .poll(async () => {
      const status = await page.request.get("/api/v1/podcasts");
      expect(status.status()).toBe(200);
      const subscriptions: Schema["PodcastSubscription"][] =
        await status.json();
      return subscriptions.find(
        (item) => item.subscription_id === subscription,
      );
    })
    .toMatchObject({
      content_revision: archive.content_revision + 1,
      refresh_pending: false,
      refresh_failed: false,
    });
  const refreshed: Schema["PodcastEpisodePage"] = await (
    await refreshedEpisodes
  ).json();
  expect(refreshed.content_revision).toBe(archive.content_revision + 1);
  expect(refreshed.total).toBe(1);
  expect(
    refreshed.items.map((item) => [item.episode_id, item.recording.id]),
  ).toEqual(archive.items.map((item) => [item.episode_id, item.recording.id]));
  expect(refreshed.items[0].recording.liked).toBe(true);
  expect(await (await refreshedProgress).json()).toEqual([retainedProgress]);
  const refreshedSources = await page.request.get(
    `/api/v1/recordings/${recording}/sources`,
  );
  expect(refreshedSources.status()).toBe(200);
  const sources: Schema["Source"][] = await refreshedSources.json();
  expect(sources.map((item) => item.id)).toEqual([
    new URL(source).pathname.split("/").at(-2),
  ]);
  expect(await recordingProgress(page, recording)).toEqual(retainedProgress);
  await expect(refresh).toBeEnabled({ timeout: 15_000 });
  await expect(page.getByText("Checking feed…", { exact: true })).toHaveCount(
    0,
  );
  await expect(
    page.getByText("Loading episodes and saved positions…", { exact: true }),
  ).toHaveCount(0);
  await expect(unlike).toBeVisible();
  await expect(resume).toBeVisible();
  expect(
    await audio.evaluate((element: HTMLAudioElement) => ({
      paused: element.paused,
      position: element.currentTime,
      source: element.currentSrc,
    })),
  ).toEqual(mediaBeforeRefresh);
  const afterRefresh = await fixtureStatus(page);
  expect(afterRefresh.enclosure_requests).toBe(
    beforeRefresh.enclosure_requests,
  );
  expect(audioRequests).toHaveLength(requestsBeforeRefresh);
  await expectNoDownload(page, recording);

  const unsubscribePath = `/api/v1/podcasts/${subscription}/unsubscribe`;
  const unsubscribeAccepted = page.waitForResponse(
    (candidate) =>
      new URL(candidate.url()).pathname === unsubscribePath &&
      candidate.request().method() === "POST",
  );
  await page.getByRole("button", { name: "Unsubscribe", exact: true }).click();
  const removed = await unsubscribeAccepted;
  expect(removed.status()).toBe(200);
  const unsubscribeRequest: Schema["UnsubscribePodcast"] = removed
    .request()
    .postDataJSON();
  const unsubscribeReceipt: Schema["PodcastUnsubscribeReceipt"] =
    await removed.json();
  expect(unsubscribeRequest).toEqual({
    request_id: expect.stringMatching(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    ),
  });
  expect(unsubscribeReceipt).toEqual({
    request_id: unsubscribeRequest.request_id,
    subscription_id: subscription,
  });
  await expect(page).toHaveURL(`${origin}/podcasts`);
  await expect(
    page.getByRole("heading", { name: "Podcasts", exact: true }),
  ).toBeVisible();
  await expect(podcast).toHaveCount(0);
  const inactive = await page.request.get("/api/v1/podcasts");
  expect(inactive.status()).toBe(200);
  expect(await inactive.json()).toEqual([]);
  const inactiveEpisodes = await page.request.get(
    `/api/v1/podcasts/${subscription}/episodes`,
  );
  expect(inactiveEpisodes.status()).toBe(404);
  const retainedSources = await page.request.get(
    `/api/v1/recordings/${recording}/sources`,
  );
  expect(retainedSources.status()).toBe(200);
  const unavailable: Schema["Source"][] = await retainedSources.json();
  expect(
    unavailable.map((item) => ({ id: item.id, ready: item.ready })),
  ).toEqual(sources.map((item) => ({ id: item.id, ready: false })));
  const likedRecordings = await page.request.get(
    "/api/v1/recordings?liked=true&limit=200",
  );
  expect(likedRecordings.status()).toBe(200);
  const retained: Schema["RecordingPage"] = await likedRecordings.json();
  expect(retained.items.find((item) => item.id === recording)).toMatchObject({
    id: recording,
    title,
    liked: true,
    available: false,
  });
  expect(await recordingProgress(page, recording)).toEqual(retainedProgress);
  await expect(audio).toHaveCount(1);
  await expect(dock.locator(".now-playing strong")).toHaveText(title);
  expect(
    await audio.evaluate((element: HTMLAudioElement) => ({
      paused: element.paused,
      position: element.currentTime,
      source: element.currentSrc,
    })),
  ).toEqual(mediaBeforeRefresh);
  const afterUnsubscribe = await fixtureStatus(page);
  expect(afterUnsubscribe.feed_requests).toBe(afterRefresh.feed_requests);
  expect(afterUnsubscribe.enclosure_requests).toBe(
    afterRefresh.enclosure_requests,
  );
  expect(audioRequests).toHaveLength(requestsBeforeRefresh);
  await expectNoDownload(page, recording);

  const played = await fixtureStatus(page);
  expect(played.enclosure_requests).toBeGreaterThan(0);
  expect(played.unexpected_requests).toBe(0);
  expect(audioRequests.length).toBeGreaterThan(0);
  expect(foreignOrigins).toEqual([]);
  expect(errors).toEqual([]);
});

async function fixtureStatus(page: Page): Promise<FixtureStatus> {
  const response = await page.request.get("/__podcast_fixture");
  expect(response.status()).toBe(200);
  return response.json();
}

async function expectNoDownload(page: Page, recording: string) {
  const response = await page.request.get(
    `/api/v1/recordings/${recording}/download`,
  );
  expect(response.status()).toBe(404);
}

async function recordingProgress(
  page: Page,
  recording: string,
): Promise<Schema["RecordingProgress"]> {
  const response = await page.request.post("/api/v1/recordings/progress", {
    headers: { origin, "x-hito-request": "1" },
    data: {
      recording_ids: [recording],
    } satisfies Schema["SavedPlaybackRequest"],
  });
  expect(response.status()).toBe(200);
  const progress: Schema["RecordingProgress"][] = await response.json();
  expect(progress).toHaveLength(1);
  return progress[0];
}

function isAudio(response: Response): boolean {
  return (
    new URL(response.url()).origin === origin &&
    audioPath.test(new URL(response.url()).pathname) &&
    response.request().method() === "GET" &&
    [200, 206].includes(response.status())
  );
}

function savedProgress(
  response: Response,
  recording: string,
  minimumMs: number,
): boolean {
  if (
    new URL(response.url()).pathname !== "/api/v1/sync" ||
    response.request().method() !== "POST"
  )
    return false;
  const batch: Schema["SyncBatch"] = response.request().postDataJSON();
  return batch.operations.some(
    ({ action }) =>
      action.kind === "progress_set" &&
      action.recording_id === recording &&
      action.position_ms >= minimumMs,
  );
}
