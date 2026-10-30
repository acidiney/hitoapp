import { randomUUID } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { expect, test, type Page } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import { artistReviewOriginals } from "./fixtures/artist-review-state";
import { audiobookPlaybackEvidence } from "./fixtures/audiobook-playback-evidence";
import {
  audiobookMediaState,
  audiobookNavigationCheck,
  audiobookPlaybackAudio,
  audiobookPlaybackPrivateValues,
  audiobookPlaybackState,
  audiobookPlaybackUsername,
  observeAudiobookMedia,
  preservedAudiobookPlayback,
} from "./fixtures/audiobook-playback-state";

test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});
test.skip(
  process.env.HITO_AUDIOBOOK_PLAYBACK_E2E !== "1",
  "Requires an opted-in fresh disposable packaged audiobook playback server",
);

test("explicit chapter playback advances to the next chapter and a bookmark selects the second duplicate while navigation preserves audio", async ({
  page,
  context,
  baseURL,
  browserName,
}) => {
  test.setTimeout(120_000);
  page.setDefaultTimeout(10_000);
  page.setDefaultNavigationTimeout(15_000);
  let stage = "fresh disposable server";
  let failed = false;
  let account = "";
  let evidence: Awaited<ReturnType<typeof audiobookPlaybackEvidence>> | null =
    null;
  const state = audiobookPlaybackState(page, () => account);
  const password = `Owned audiobook playback password ${randomUUID()}`;
  const privateValues = audiobookPlaybackPrivateValues(password);
  const checks = {
    fresh: false,
    prepared: false,
    chapter_zero: false,
    chapter_advances: false,
    natural_next: false,
    bookmark_exact: false,
    duplicate_queue: false,
    navigation_keeps_audio: false,
    paused: false,
    private_state: false,
    originals: false,
    downloads: false,
    audio_bytes: false,
    transport: false,
    console: false,
  };
  const diagnostics = {
    first_ms: -1,
    first_start_ms: -1,
    near_end_ms: -1,
    bookmark_ms: -1,
    bookmark_seek_delta_ms: -1,
    paused_ms: -1,
    natural_endings: 0,
    remaining_queue_rows: 0,
    navigation_step: 0,
    return_named_links: 0,
    return_owned_links: 0,
    private_reason: 0,
    private_detail: [] as number[],
    private_final_a: false,
    private_final_b: false,
    progress_before_snapshot: 0,
    progress_after_snapshot: 0,
  };
  const rememberCookies = async () =>
    privateValues.push(
      ...(await context.cookies()).map((cookie) => cookie.value),
    );
  try {
    if (
      baseURL !== "http://localhost:8080" ||
      process.env.PLAYWRIGHT_NO_COPY_PROMPT !== "1"
    )
      throw new Error(
        "The isolated audiobook playback privacy configuration is required",
      );
    evidence = await audiobookPlaybackEvidence(
      context,
      baseURL,
      browserName,
      privateValues,
    );
    checks.fresh = (
      await state.read<Schema<"SetupStatus">>("/api/v1/setup")
    ).needs_setup;
    expect(checks.fresh).toBe(true);
    const originals = await artistReviewOriginals();
    await page.goto("/");
    await page
      .getByLabel("Username", { exact: true })
      .fill(audiobookPlaybackUsername);
    await page.getByLabel("Password", { exact: true }).fill(password);
    await page
      .getByRole("button", { name: "Create administrator", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    await rememberCookies();
    const user = await state.read<Schema<"User">>("/api/v1/me");
    account = user.id;
    await page.evaluate(() =>
      console.info("owned audiobook playback console probe"),
    );

    stage = "owned originals and private audiobook preparation";
    const saved = await state.prepare(user);
    privateValues.push(...saved.privateValues);
    const before = await state.snapshot(saved);
    const audio = await audiobookPlaybackAudio(before);
    const absence = await state.absence(saved);
    checks.prepared =
      before.book.chapters.length === 3 &&
      isDeepStrictEqual(
        before.book.chapters.map((chapter) => chapter.entry_id),
        saved.entries,
      ) &&
      isDeepStrictEqual(
        before.book.chapters.map((chapter) => chapter.recording_id),
        [saved.ids[0], saved.ids[1], saved.ids[0]],
      ) &&
      before.book.chapters.every(
        (chapter) => chapter.available && chapter.duration_ms === 90_000,
      ) &&
      before.book.bookmarks.length === 1 &&
      before.book.bookmarks[0].id === saved.marker &&
      before.book.bookmarks[0].entry_id === saved.entries[2] &&
      before.book.bookmarks[0].recording_id === saved.ids[0] &&
      before.book.bookmarks[0].position_ms === 35_125 &&
      before.book.bookmarks[0].label === saved.label &&
      before.metadata[0].title.personal === saved.privateValues[0] &&
      before.metadata[0].artist.personal === saved.privateValues[1] &&
      before.catalog.items.every((recording) => recording.liked) &&
      before.progress.some(
        (item) =>
          item.recording_id === saved.ids[0] &&
          item.position_ms === 73_400 &&
          item.speed === 1.25,
      ) &&
      before.progress.some(
        (item) =>
          item.recording_id === saved.ids[1] &&
          item.position_ms === 800 &&
          item.speed === 1.5,
      );
    expect(checks.prepared).toBe(true);
    await page.goto(`/audiobooks/${saved.playlist}`);
    await expect(
      page.getByRole("heading", {
        name: before.book.playlist.name,
        exact: true,
      }),
    ).toBeVisible();
    await expect(
      page
        .getByRole("list", { name: "Chapters", exact: true })
        .getByRole("listitem"),
    ).toHaveCount(3);
    await page.waitForLoadState("networkidle", { timeout: 10_000 });
    await expect.poll(() => evidence!.pending()).toBe(0);
    expect(
      await page.locator("audio").evaluateAll(
        (elements) =>
          elements.length === 1 &&
          elements.every((element) => {
            const audio = element as HTMLAudioElement;
            return (
              audio.paused &&
              audio.currentTime === 0 &&
              !audio.currentSrc &&
              !audio.getAttribute("src")
            );
          }),
      ),
    ).toBe(true);
    await evidence.arm({
      account,
      book: before.book,
      recordings: saved.ids,
      sources: before.sources,
      audio,
    });

    stage = "explicit chapter from beginning";
    const chapter = page.getByRole("button", {
      name: "Play chapter 1 from beginning",
      exact: true,
    });
    await expect(chapter).toBeVisible();
    const observed = await observeAudiobookMedia(page);
    const document = await page.evaluate(() => performance.timeOrigin);
    await chapter.click();
    await expect
      .poll(async () => {
        const current = await audiobookMediaState(page, audio[0].path);
        return (
          current.exact &&
          current.playing &&
          current.position > 0.2 &&
          current.position < 3 &&
          Math.abs(current.duration - 90) < 0.1
        );
      })
      .toBe(true);
    const first = await audiobookMediaState(page, audio[0].path);
    diagnostics.first_ms = Math.round(first.position * 1000);
    const firstObserved = await observed();
    diagnostics.first_start_ms = firstObserved.starts[0] ?? -1;
    checks.chapter_zero =
      firstObserved.starts.length > 0 && firstObserved.starts[0] < 250;
    expect(checks.chapter_zero).toBe(true);
    await expect
      .poll(async () => {
        const current = await audiobookMediaState(page, audio[0].path);
        return (
          current.exact &&
          current.playing &&
          current.position - first.position > 0.3
        );
      })
      .toBe(true);
    checks.chapter_advances = true;
    await assertQueue(
      page,
      before.book.chapters.map((chapter) => chapter.title),
      0,
    );
    await expect.poll(() => evidence!.audio().distinct).toBe(2);
    await expect.poll(() => evidence!.pending()).toBe(0);

    stage = "UI seek followed by natural chapter advancement";
    const seek = page
      .locator("footer.player")
      .getByRole("slider", { name: "Seek", exact: true });
    await evidence.checkpoint(1);
    await seek.focus();
    await seek.press("Home");
    for (let index = 0; index < 9; index++) await seek.press("PageUp");
    await evidence.checkpoint(2);
    const nearEnd = await audiobookMediaState(page, audio[0].path);
    diagnostics.near_end_ms = Math.round(nearEnd.position * 1000);
    expect(
      nearEnd.exact &&
        nearEnd.playing &&
        nearEnd.position >= 80 &&
        nearEnd.position < 89,
    ).toBe(true);
    await expect
      .poll(
        async () => {
          const current = await audiobookMediaState(page, audio[1].path);
          return (
            current.exact &&
            current.playing &&
            current.position > 0.2 &&
            current.position < 4 &&
            Math.abs(current.duration - 90) < 0.1
          );
        },
        { timeout: 15_000 },
      )
      .toBe(true);
    await assertQueue(
      page,
      before.book.chapters.map((chapter) => chapter.title),
      1,
    );
    diagnostics.natural_endings = (await observed()).natural_endings;
    checks.natural_next = diagnostics.natural_endings === 1;
    expect(checks.natural_next).toBe(true);
    await expect.poll(() => evidence!.pending()).toBe(0);
    await evidence.checkpoint(3);

    stage = "exact bookmark at the second duplicate occurrence";
    const marker = page
      .getByRole("region", { name: "Bookmarks", exact: true })
      .getByRole("listitem");
    await expect(
      marker.getByText(`Chapter 3: ${before.book.chapters[2].title}`, {
        exact: true,
      }),
    ).toBeVisible();
    await evidence.checkpoint(4);
    const bookmarkSeek = await evidence.seekCheckpoint(
      page,
      audio[0].path,
      35_125,
    );
    await marker
      .getByRole("button", { name: "Play bookmark", exact: true })
      .click();
    await expect
      .poll(async () => {
        const current = await audiobookMediaState(page, audio[0].path);
        return (
          current.exact &&
          current.playing &&
          current.position >= 35.125 &&
          current.position < 38 &&
          Math.abs(current.duration - 90) < 0.1
        );
      })
      .toBe(true);
    const bookmarked = await audiobookMediaState(page, audio[0].path);
    diagnostics.bookmark_ms = Math.round(bookmarked.position * 1000);
    await assertQueue(
      page,
      before.book.chapters.map((chapter) => chapter.title),
      2,
    );
    const bookmarkObserved = await observed();
    diagnostics.bookmark_seek_delta_ms = Math.min(
      ...bookmarkObserved.seeks.map((position) => Math.abs(position - 35_125)),
    );
    checks.bookmark_exact =
      bookmarkObserved.seeks.some(
        (position) => Math.abs(position - 35_125) <= 25,
      ) && !bookmarkObserved.overflow;
    expect(checks.bookmark_exact).toBe(true);
    await expect.poll(() => evidence!.pending()).toBe(0);
    await expect.poll(() => bookmarkSeek.ready()).toBe(true);
    await bookmarkSeek.finish();
    await evidence.checkpoint(5);
    await evidence.checkpoint(6);
    await expect
      .poll(async () => {
        const current = await audiobookMediaState(page, audio[0].path);
        return (
          current.exact &&
          current.playing &&
          current.position - bookmarked.position > 0.3
        );
      })
      .toBe(true);
    diagnostics.remaining_queue_rows = await removeEarlierChapter(
      page,
      before.book.chapters.map((chapter) => chapter.title),
    );
    checks.duplicate_queue = diagnostics.remaining_queue_rows === 2;
    await expect
      .poll(async () => {
        const current = await audiobookMediaState(page, audio[0].path);
        return (
          current.exact &&
          current.playing &&
          current.position - bookmarked.position > 0.3
        );
      })
      .toBe(true);
    await evidence.checkpoint(7);

    stage =
      "Inertia navigation retains audio and UI pause saves recording progress";
    const navigation = await audiobookNavigationCheck(
      page,
      audio[0].path,
      document,
      bookmarked.position,
    );
    await evidence.checkpoint(8);
    diagnostics.navigation_step = 1;
    await page
      .getByRole("navigation", { name: "Breadcrumb" })
      .getByRole("link", { name: "Audiobooks", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Audiobooks", exact: true }),
    ).toBeVisible();
    diagnostics.navigation_step = 2;
    await evidence.checkpoint(9);
    diagnostics.return_named_links = await page
      .getByRole("link", {
        name: before.book.playlist.name,
        exact: true,
      })
      .count();
    const returnLink = page
      .getByRole("main")
      .getByRole("link", {
        name: before.book.playlist.name,
        exact: true,
      })
      .and(page.locator(`a[href="/audiobooks/${saved.playlist}"]`));
    diagnostics.return_owned_links = await returnLink.count();
    await expect(returnLink).toHaveCount(1);
    diagnostics.navigation_step = 3;
    await evidence.checkpoint(10);
    await returnLink.click();
    await expect(
      page.getByRole("heading", {
        name: before.book.playlist.name,
        exact: true,
      }),
    ).toBeVisible();
    diagnostics.navigation_step = 4;
    await evidence.checkpoint(11);
    checks.navigation_keeps_audio = await navigation();
    diagnostics.navigation_step = 5;
    expect(checks.navigation_keeps_audio).toBe(true);
    await evidence.checkpoint(12);
    const pause = await evidence.pauseCheckpoint(page, audio[0].path);
    await page
      .locator("footer.player")
      .getByRole("button", { name: "Pause", exact: true })
      .click();
    const paused = await pause();
    diagnostics.paused_ms = Math.round(paused.position * 1000);
    checks.paused =
      paused.exact && paused.position >= 35.125 && paused.position < 45;
    await evidence.checkpoint(13);

    stage = "private collection and original audio preservation";
    diagnostics.progress_before_snapshot = evidence.counts.progress_accepted;
    const after = await state.snapshot(saved);
    diagnostics.progress_after_snapshot = evidence.counts.progress_accepted;
    diagnostics.private_final_a = after.progress.some(
      (item) =>
        item.recording_id === saved.ids[0] &&
        item.position_ms >= 35_125 &&
        item.position_ms < 45_000 &&
        item.speed === 1,
    );
    diagnostics.private_final_b = after.progress.some(
      (item) =>
        item.recording_id === saved.ids[1] &&
        item.position_ms > 0 &&
        item.position_ms < 10_000 &&
        item.speed === 1,
    );
    checks.private_state =
      preservedAudiobookPlayback(
        before,
        after,
        evidence.accepted(),
        (reason, detail) => {
          diagnostics.private_reason = reason;
          diagnostics.private_detail = detail;
        },
      ) &&
      diagnostics.private_final_a &&
      diagnostics.private_final_b;
    checks.originals = isDeepStrictEqual(
      await artistReviewOriginals(),
      originals,
    );
    checks.downloads = isDeepStrictEqual(await state.absence(saved), absence);
    const audit = evidence.audio();
    checks.audio_bytes =
      audit.unexplained_violations === 0 &&
      audit.unpairedProbes === 0 &&
      audit.distinct === 2;
    expect(
      Object.entries(checks)
        .filter(([key]) => key !== "transport" && key !== "console")
        .every(([, value]) => value),
    ).toBe(true);
  } catch {
    failed = true;
  } finally {
    try {
      await rememberCookies();
    } catch {
      failed = true;
    }
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      await evidence?.checkpoint(14);
      await Promise.race([
        context.close(),
        new Promise<never>((_, reject) => {
          timer = setTimeout(
            () => reject(new Error("The owned browser cleanup timed out")),
            10_000,
          );
        }),
      ]);
    } catch {
      failed = true;
    } finally {
      clearTimeout(timer);
    }
    try {
      await evidence?.finish();
      await evidence?.checkpoint(15);
    } catch {
      failed = true;
    }
  }
  if (evidence) {
    const counts = evidence.counts;
    checks.transport =
      counts.forbidden + counts.invalid + counts.page_errors === 0 &&
      evidence.audio().unexplained_workflow_failures === 0 &&
      counts.preparation_requests + counts.workflow_requests <= 300 &&
      counts.book_reads === 2 &&
      counts.progress_submitted === counts.progress_accepted &&
      counts.progress_accepted > 0;
    checks.console =
      counts.private_console + counts.malformed_console === 0 &&
      counts.console_bytes <= 524_288 &&
      counts.playback_started === 2 &&
      counts.playback_finished === 2 &&
      counts.chapter_handoffs === 1 &&
      counts.bookmark_handoffs === 1 &&
      counts.probe_text > 0 &&
      counts.probe_snapshot > 0;
  }
  if (!Object.values(checks).every(Boolean)) failed = true;
  const report = JSON.stringify({
    stage,
    failed,
    api_requests: state.requests(),
    checks,
    diagnostics,
    transport: evidence?.counts ?? {},
    audio: evidence?.audio() ?? {},
  });
  const detail = evidence?.diagnostics();
  const lines = detail
    ? [
        `audiobook_playback_requests ${JSON.stringify({ overflow: detail.overflow, requests: detail.requests, checkpoints: detail.checkpoints, seek: detail.seek })}`,
        `audiobook_playback_browser ${report}`,
      ]
    : [`audiobook_playback_browser ${report}`];
  const sizes = lines.map((line) => Buffer.byteLength(line));
  if (
    sizes.some((size) => size > 4096) ||
    sizes.reduce((sum, size) => sum + size, 0) > 8192
  )
    throw new Error(
      "The audiobook playback evidence exceeded its output bound",
    );
  lines.forEach((line) => console.log(line));
  if (failed)
    throw new Error(
      `Audiobook playback browser verification failed at ${stage}`,
    );
});

async function assertQueue(page: Page, titles: string[], selected: number) {
  await page
    .locator("footer.player")
    .getByRole("button", { name: "Open queue", exact: true })
    .click();
  await expect(page).toHaveURL(/\/queue$/);
  await expect(
    page
      .getByRole("region", { name: "Now playing", exact: true })
      .locator("strong"),
  ).toHaveText(titles[selected]);
  const rows = page.locator("ol.queue-tracks > li");
  await expect(rows).toHaveCount(titles.length - selected - 1);
  for (const [index, title] of titles.slice(selected + 1).entries())
    await expect(rows.nth(index).locator("strong")).toHaveText(title);
  await page.goBack();
  await expect(page.locator("footer.player")).toBeVisible();
}

/** Removes the first chapter from the Played section and returns how many rows the queue then shows. */
async function removeEarlierChapter(page: Page, titles: string[]) {
  await page
    .locator("footer.player")
    .getByRole("button", { name: "Open queue", exact: true })
    .click();
  await expect(page).toHaveURL(/\/queue$/);
  await page.getByText(/^Played \(/).click();
  await page
    .getByRole("button", {
      name: `Remove ${titles[0]} from queue`,
      exact: true,
    })
    .click();
  const rows = page.locator("ol.queue-tracks > li");
  await expect(rows).toHaveCount(1);
  await expect(rows.first().locator("strong")).toHaveText(titles[1]);
  await expect(
    page
      .getByRole("region", { name: "Now playing", exact: true })
      .locator("strong"),
  ).toHaveText(titles[2]);
  const count = (await rows.count()) + 1;
  await page.goBack();
  await expect(page.locator("footer.player")).toBeVisible();
  return count;
}
