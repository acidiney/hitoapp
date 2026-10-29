import { randomUUID } from "node:crypto";
import { expect, test } from "@playwright/test";
import { captureConsolePrivacy } from "./fixtures/console-privacy";
import {
  fixtureStatus,
  prepareOwnedOriginal,
  progress,
  read,
  stopUpstream,
  type Schema,
} from "./fixtures/podcast-download";
import { openLibrarySection } from "./fixtures/library-sections";

const origin = "http://localhost:8080";
const feedTitle = "Owned audio feed";
const episodeTitle = "Owned playable episode";
const personalTitle = "Saved podcast personal title";
const password = "owned podcast download browser password";
const uuid =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

test.use({
  launchOptions: { args: [] },
  trace: "off",
  screenshot: "off",
  video: "off",
});
test.skip(
  process.env.HITO_PODCAST_DOWNLOAD_E2E !== "1",
  "Requires a fresh disposable podcast_browser host with fixture stop controls",
);

test("an explicit RSS download survives unsubscribe and upstream shutdown, then removal preserves the original and personal state", async ({
  page,
  context,
  baseURL,
}) => {
  test.setTimeout(90_000);
  expect(baseURL).toBe(origin);
  const setup = await page.request.get("/api/v1/setup");
  expect(setup.status()).toBe(200);
  expect(await setup.json()).toEqual({ needs_setup: true });
  const initial = await fixtureStatus(page);
  expect(initial.kind).toBe("hito-owned-podcast-browser");
  expect(initial.feed_requests + initial.enclosure_requests).toBe(0);
  expect(initial.upstream_closed).toBe(false);
  expect(new URL(initial.feed_url).origin).toMatch(
    /^http:\/\/127\.0\.0\.1:\d+$/,
  );
  let pageErrors = 0;
  let foreignRequests = 0;
  let privacyFailures = 0;
  let leakedConsole = 0;
  let browserAudioRequests = 0;
  const pendingConsole = new Set<Promise<void>>();
  const creations: string[] = [];
  const removals: string[] = [];
  let downloadPath = "";
  page.on("pageerror", () => pageErrors++);
  page.on("request", (request) => {
    const url = new URL(request.url());
    if (/^\/api\/v1\/sources\/[0-9a-f-]+\/audio$/.test(url.pathname))
      browserAudioRequests++;
    if (request.method() === "POST" && url.pathname === downloadPath)
      creations.push(
        (request.postDataJSON() as Schema["DownloadRequest"]).request_id,
      );
    if (
      request.method() === "DELETE" &&
      /^\/api\/v1\/downloads\/[0-9a-f-]+$/.test(url.pathname)
    )
      removals.push(url.pathname);
  });
  await captureConsolePrivacy(
    context,
    (text) => {
      if (
        [
          password,
          personalTitle,
          initial.feed_url,
          "podcast-private-enclosure-canary",
        ].some((privateValue) => text.includes(privateValue))
      )
        leakedConsole++;
    },
    () => privacyFailures++,
    pendingConsole,
  );
  await context.route("**/*", async (route) => {
    if (new URL(route.request().url()).origin !== origin) {
      foreignRequests++;
      await route.abort();
    } else await route.continue();
  });

  try {
    await page.goto("/");
    await page.getByLabel("Username", { exact: true }).fill("admin");
    await page.getByLabel("Password", { exact: true }).fill(password);
    await page
      .getByRole("button", { name: "Create administrator", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    const original = await prepareOwnedOriginal(page);
    const navigation = page.getByRole("navigation", {
      name: "Main navigation",
    });
    await openLibrarySection(page, "Podcasts");
    await page
      .getByLabel("Feed address", { exact: true })
      .fill(initial.feed_url);
    const subscribeResponse = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === "/api/v1/podcasts" &&
        response.request().method() === "POST",
    );
    await page.getByRole("button", { name: "Subscribe", exact: true }).click();
    expect((await subscribeResponse).status()).toBe(202);
    const podcast = page.getByRole("link", { name: feedTitle, exact: true });
    await expect(podcast).toBeVisible({ timeout: 15_000 });
    const podcastPath = await podcast.getAttribute("href");
    if (!podcastPath)
      throw new Error("The owned podcast detail link must be present");
    const subscription = podcastPath.split("/").at(-1)!;
    await podcast.click();
    const episodes = page.getByRole("list", { name: "Episodes", exact: true });
    await expect(episodes.getByRole("listitem")).toHaveCount(1);
    await episodes
      .getByRole("button", { name: `Like ${episodeTitle}`, exact: true })
      .click();
    await expect(
      episodes.getByRole("button", {
        name: `Unlike ${episodeTitle}`,
        exact: true,
      }),
    ).toBeVisible();
    const archive = await read<Schema["PodcastEpisodePage"]>(
      page,
      `/api/v1/podcasts/${subscription}/episodes`,
    );
    expect(archive.items).toHaveLength(1);
    const recording = archive.items[0].recording;
    expect(recording.liked).toBe(true);
    expect(
      recording.id !== original.recording.id &&
        recording.library_id !== original.recording.library_id,
    ).toBe(true);
    downloadPath = `/api/v1/recordings/${recording.id}/download`;
    const remoteSources = await read<Schema["Source"][]>(
      page,
      `/api/v1/recordings/${recording.id}/sources`,
    );
    expect(remoteSources).toHaveLength(1);
    expect(
      remoteSources[0].provider === "podcast" && remoteSources[0].ready,
    ).toBe(true);
    await episodes
      .getByRole("link", { name: `Details for ${episodeTitle}`, exact: true })
      .click();
    await expect(page).toHaveURL(`${origin}/recordings/${recording.id}`);
    await page.getByLabel("Track title", { exact: true }).fill(personalTitle);
    await page
      .getByRole("button", { name: "Save personal title", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: personalTitle, exact: true }),
    ).toBeVisible();
    const operation = randomUUID();
    const savedProgress = await page.request.post("/api/v1/sync", {
      headers: { origin, "x-hito-request": "1" },
      data: {
        device_id: "podcast-download-browser",
        operations: [
          {
            id: operation,
            action: {
              kind: "progress_set",
              recording_id: recording.id,
              position_ms: 700,
              speed: 1.25,
            },
          },
        ],
      } satisfies Schema["SyncBatch"],
    });
    expect(savedProgress.status()).toBe(200);
    expect(
      (
        (await savedProgress.json()) as Schema["SyncReceipt"]
      ).acknowledged.includes(operation),
    ).toBe(true);
    const metadata = await read<Schema["RecordingMetadata"]>(
      page,
      `/api/v1/recordings/${recording.id}/metadata`,
    );
    expect(metadata.title.personal === personalTitle).toBe(true);
    const beforeProgress = await progress(page, recording.id, origin);
    expect(beforeProgress.position_ms).toBe(700);
    const audio = page.locator("audio[data-hito-player]");
    await expect(audio).toHaveCount(1);
    expect(await audio.getAttribute("src")).toBeNull();
    const section = page.getByRole("region", { name: "Download", exact: true });
    const download = section.getByRole("button", {
      name: "Download",
      exact: true,
    });
    await expect(download).toBeEnabled();
    expect((await page.request.get(downloadPath)).status()).toBe(404);
    expect(browserAudioRequests).toBe(0);
    expect((await fixtureStatus(page)).enclosure_requests).toBe(0);
    const createResponse = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === downloadPath &&
        response.request().method() === "POST",
    );
    await download.click();
    const accepted = await createResponse;
    expect(accepted.status()).toBe(202);
    const body: Schema["DownloadRequest"] = accepted.request().postDataJSON();
    const receipt: Schema["DownloadReceipt"] = await accepted.json();
    expect(body).toEqual({ request_id: expect.stringMatching(uuid) });
    expect(receipt).toEqual({
      request_id: body.request_id,
      intent_id: expect.stringMatching(uuid),
      recording_id: recording.id,
    });
    await expect(section.getByRole("status")).toHaveText(
      "Saved on this server",
      { timeout: 30_000 },
    );
    const ready = await read<Schema["DownloadStatus"]>(page, downloadPath);
    expect(
      ready.status === "ready" && ready.intent_id === receipt.intent_id,
    ).toBe(true);
    if (!ready.source_id)
      throw new Error(
        "The completed RSS download must expose its managed source",
      );
    const managedAudio = `/api/v1/sources/${ready.source_id}/audio`;
    expect(ready.bytes_received).toBe(original.audio.length);
    expect(ready.total_bytes).toBe(original.audio.length);
    expect(creations).toEqual([body.request_id]);
    expect(browserAudioRequests).toBe(0);
    expect(await audio.getAttribute("src")).toBeNull();
    expect(
      await audio.evaluate((element: HTMLAudioElement) => element.paused),
    ).toBe(true);
    expect(await progress(page, recording.id, origin)).toEqual(beforeProgress);
    await original.assertRetained(page);

    await openLibrarySection(page, "Podcasts");
    await page.getByRole("link", { name: feedTitle, exact: true }).click();
    const unsubscribeResponse = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname ===
          `/api/v1/podcasts/${subscription}/unsubscribe` &&
        response.request().method() === "POST",
    );
    await page
      .getByRole("button", { name: "Unsubscribe", exact: true })
      .click();
    const unsubscribed = await unsubscribeResponse;
    expect(unsubscribed.status()).toBe(200);
    const unsubscribeBody: Schema["UnsubscribePodcast"] = unsubscribed
      .request()
      .postDataJSON();
    expect(await unsubscribed.json()).toEqual({
      request_id: unsubscribeBody.request_id,
      subscription_id: subscription,
    });
    await expect(page).toHaveURL(`${origin}/podcasts`);
    await expect(
      page.getByRole("link", { name: feedTitle, exact: true }),
    ).toHaveCount(0);
    expect(
      await read<Schema["PodcastSubscription"][]>(page, "/api/v1/podcasts"),
    ).toEqual([]);
    expect(
      (
        await page.request.get(`/api/v1/podcasts/${subscription}/episodes`)
      ).status(),
    ).toBe(404);
    expect(await read<Schema["DownloadStatus"]>(page, downloadPath)).toEqual(
      ready,
    );
    expect(await progress(page, recording.id, origin)).toEqual(beforeProgress);
    const stopped = await stopUpstream(page, origin);
    expect(stopped.feed_requests).toBe(1);
    expect(stopped.enclosure_requests).toBe(1);
    expect(stopped.unexpected_requests).toBe(0);
    expect((await fixtureStatus(page)).upstream_closed).toBe(true);

    await navigation
      .getByRole("link", { name: "Library", exact: true })
      .click();
    const delivered = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === managedAudio &&
        response.request().method() === "GET" &&
        [200, 206].includes(response.status()),
    );
    await page
      .getByRole("button", { name: `Play ${personalTitle}`, exact: true })
      .click();
    expect((await delivered).headers()["content-type"]).toBe("audio/wav");
    const position = () =>
      audio.evaluate((element: HTMLAudioElement) => element.currentTime);
    await expect.poll(position).toBeGreaterThan(2);
    expect(
      new URL(
        await audio.evaluate((element: HTMLAudioElement) => element.currentSrc),
      ).pathname,
    ).toBe(managedAudio);
    const bytes = await page.request.get(managedAudio);
    expect(bytes.status()).toBe(200);
    expect((await bytes.body()).equals(original.audio)).toBe(true);
    const dock = page.getByRole("contentinfo", {
      name: "Audio player",
      exact: true,
    });
    await dock.getByRole("button", { name: "Pause", exact: true }).click();
    await expect
      .poll(() => audio.evaluate((element: HTMLAudioElement) => element.paused))
      .toBe(true);
    const pausedMs = Math.floor((await position()) * 1000);
    await expect
      .poll(async () =>
        Math.abs(
          (await progress(page, recording.id, origin)).position_ms - pausedMs,
        ),
      )
      .toBeLessThan(250);
    const retainedProgress = await progress(page, recording.id, origin);
    expect((await fixtureStatus(page)).enclosure_requests).toBe(
      stopped.enclosure_requests,
    );

    await page
      .getByRole("link", { name: `Details for ${personalTitle}`, exact: true })
      .click();
    await expect(section.getByRole("status")).toHaveText(
      "Saved on this server",
    );
    const removePath = `/api/v1/downloads/${ready.intent_id}`;
    const removalResponse = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === removePath &&
        response.request().method() === "DELETE",
    );
    await section
      .getByRole("button", { name: "Remove download", exact: true })
      .click();
    expect((await removalResponse).status()).toBe(204);
    await expect(
      section.getByRole("button", { name: "Download", exact: true }),
    ).toBeEnabled();
    expect((await page.request.get(downloadPath)).status()).toBe(404);
    expect((await page.request.get(managedAudio)).status()).toBe(404);
    const remaining = await read<Schema["Source"][]>(
      page,
      `/api/v1/recordings/${recording.id}/sources`,
    );
    expect(
      remaining.map((source) => ({
        id: source.id,
        provider: source.provider,
        ready: source.ready,
      })),
    ).toEqual([{ id: remoteSources[0].id, provider: "podcast", ready: false }]);
    const retained = await read<Schema["RecordingPage"]>(
      page,
      "/api/v1/recordings?liked=true&limit=200",
    );
    expect(
      retained.items.find((item) => item.id === recording.id),
    ).toMatchObject({
      id: recording.id,
      library_id: recording.library_id,
      title: personalTitle,
      liked: true,
      available: false,
    });
    expect(
      JSON.stringify(
        await read<Schema["RecordingMetadata"]>(
          page,
          `/api/v1/recordings/${recording.id}/metadata`,
        ),
      ) === JSON.stringify(metadata),
    ).toBe(true);
    expect(await progress(page, recording.id, origin)).toEqual(
      retainedProgress,
    );
    await original.assertRetained(page);
    expect(creations).toEqual([body.request_id]);
    expect(removals).toEqual([removePath]);
    const final = await fixtureStatus(page);
    expect(final.feed_requests).toBe(stopped.feed_requests);
    expect(final.enclosure_requests).toBe(stopped.enclosure_requests);
    expect(final.unexpected_requests).toBe(0);
    expect(final.upstream_closed).toBe(true);
    await Promise.all([...pendingConsole]);
    expect(pageErrors + foreignRequests + privacyFailures + leakedConsole).toBe(
      0,
    );
  } finally {
    await stopUpstream(page, origin);
    await page.close();
  }
});
