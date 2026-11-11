import { expect, test } from "@playwright/test";
import { openSharedAdminSession } from "./fixtures/admin-session";
import {
  advancing,
  connectFixtures,
  convertedResponse,
  dock,
  mediaState,
  playbackSettings,
  playFixture,
  seek,
  verifyConverted,
  observeQualityErrors,
  ownedFixtureSources,
  originalResponse,
  verifyOriginal,
  verifyUnsupportedConverted,
  assertNoQualityDownloads,
} from "./fixtures/streaming-quality";

test.skip(
  process.env.HITO_STREAMING_QUALITY_ACCEPTANCE !== "1",
  "Run explicitly against the isolated streaming-quality server",
);

test(
  "streaming quality preserves the full timeline through seeks, navigation and reload",
  { tag: "@webkit-aac" },
  async ({ page, baseURL }) => {
    const errors = await observeQualityErrors(page);
    await openSharedAdminSession(page);
    await connectFixtures(page, baseURL!);
    const fixtures = await ownedFixtureSources(page);
    await assertNoQualityDownloads(
      page,
      fixtures.map((fixture) => fixture.recording),
    );
    await page.goto("/settings?section=playback");
    const quality = page.getByRole("button", { name: /^Streaming quality:/ });
    await expect(quality).toHaveAccessibleName("Streaming quality: Original");
    await quality.click();
    await expect(page.getByRole("menuitem")).toHaveCount(3);
    await page.getByRole("menuitem", { name: /^High/ }).click();
    await page.reload();
    await expect(quality).toHaveAccessibleName(/High/);

    const high = convertedResponse(page, "high");
    await playFixture(page, "Fixture 440 Hz");
    await verifyConverted(page, await high, "high");
    await advancing(page);

    const retained = await mediaState(page);
    await (await playbackSettings(page)).selectOption("saver");
    expect((await mediaState(page)).source).toBe(retained.source);
    await advancing(page);

    const saver = convertedResponse(page, "saver");
    await playFixture(page, "Fixture 660 Hz");
    await verifyConverted(page, await saver, "saver");
    await advancing(page);

    const forward = convertedResponse(page, "saver", 44_000, 46_000);
    const slider = seek(page);
    const bounds = await slider.boundingBox();
    expect(bounds).not.toBeNull();
    await slider.click({
      position: { x: bounds!.width / 2, y: bounds!.height / 2 },
    });
    await verifyConverted(page, await forward, "saver");
    const forwardState = await advancing(page);
    expect(forwardState.absolute).toBeGreaterThan(44);
    expect(forwardState.position).toBeLessThan(10);

    const reverse = convertedResponse(page, "saver", 21_000, 24_000);
    await slider.click({
      position: { x: bounds!.width / 4, y: bounds!.height / 2 },
    });
    await verifyConverted(page, await reverse, "saver");
    const reverseState = await advancing(page);
    expect(reverseState.absolute).toBeGreaterThan(21);
    expect(reverseState.absolute).toBeLessThan(30);
    expect(reverseState.position).toBeLessThan(10);

    await dock(page)
      .getByRole("button", { name: "Pause", exact: true })
      .click();
    await expect.poll(async () => (await mediaState(page)).paused).toBe(true);
    const paused = await mediaState(page);
    await page.waitForTimeout(700);
    expect(
      Math.abs((await mediaState(page)).position - paused.position),
    ).toBeLessThan(0.2);
    await dock(page).getByRole("button", { name: "Play", exact: true }).click();
    await advancing(page);

    const documentOrigin = await page.evaluate(() => performance.timeOrigin);
    const beforeNavigation = await mediaState(page);
    await page
      .getByRole("navigation", { name: "Main navigation" })
      .getByRole("link", { name: "Home", exact: true })
      .click();
    expect(await page.evaluate(() => performance.timeOrigin)).toBe(
      documentOrigin,
    );
    expect((await mediaState(page)).source).toBe(beforeNavigation.source);
    await advancing(page);

    await dock(page)
      .getByRole("button", { name: "Pause", exact: true })
      .click();
    await expect.poll(async () => (await mediaState(page)).paused).toBe(true);
    const checkpoint = Number(await seek(page).inputValue());
    await page.reload();
    await expect(dock(page)).toContainText("Fixture 660 Hz");
    await expect
      .poll(async () =>
        Math.abs(Number(await seek(page).inputValue()) - checkpoint),
      )
      .toBeLessThan(1.1);
    await expect(seek(page)).toHaveAttribute("max", "90");
    const restored = convertedResponse(
      page,
      "saver",
      (checkpoint - 1.1) * 1000,
      (checkpoint + 1.1) * 1000,
    );
    await dock(page).getByRole("button", { name: "Play", exact: true }).click();
    await verifyConverted(page, await restored, "saver");
    const resumed = await advancing(page);
    expect(resumed.absolute).toBeGreaterThan(checkpoint - 1.1);
    expect(resumed.absolute).toBeLessThan(checkpoint + 8);

    await expect(await playbackSettings(page)).toHaveValue("saver");
    expect(
      await page.evaluate(() => localStorage.getItem("hito:stream-quality")),
    ).toBe("saver");
    await assertNoQualityDownloads(
      page,
      fixtures.map((fixture) => fixture.recording),
    );
    expect(errors.javascript).toEqual([]);
    expect(errors.media).toEqual([]);
    expect(errors.unexpectedRequests).toEqual([]);
  },
);

test(
  "unsupported converted AAC explicitly retries the same Original source and preserves playback",
  { tag: "@chromium-original-fallback" },
  async ({ page, baseURL }, testInfo) => {
    const errors = await observeQualityErrors(page);
    await openSharedAdminSession(page);
    await connectFixtures(page, baseURL!);
    const fixtures = await ownedFixtureSources(page);
    await assertNoQualityDownloads(
      page,
      fixtures.map((fixture) => fixture.recording),
    );
    await page.goto("/settings?section=playback");
    const quality = page.getByRole("button", { name: /^Streaming quality:/ });
    await expect(quality).toHaveAccessibleName("Streaming quality: Original");
    await quality.click();
    await page.getByRole("menuitem", { name: /^High/ }).click();
    await page.reload();
    await expect(quality).toHaveAccessibleName(/High/);

    const converted = convertedResponse(page, "high");
    const fallback = originalResponse(page, fixtures[0].path);
    await playFixture(page, "Fixture 440 Hz");
    const attempted = await verifyUnsupportedConverted(
      page,
      await converted,
      fixtures[0],
      "high",
    );
    const initial = await verifyOriginal(page, await fallback, fixtures[0]);
    await advancing(page);
    await expect
      .poll(() =>
        errors.media.some(
          (error) =>
            error.source === attempted.source &&
            error.code === 4 &&
            error.trusted,
        ),
      )
      .toBe(true);

    const retained = await mediaState(page);
    await (await playbackSettings(page)).selectOption("saver");
    expect((await mediaState(page)).source).toBe(retained.source);
    await advancing(page);
    const secondConverted = convertedResponse(page, "saver");
    const secondOriginal = originalResponse(page, fixtures[1].path);
    await playFixture(page, "Fixture 660 Hz");
    const secondAttempt = await verifyUnsupportedConverted(
      page,
      await secondConverted,
      fixtures[1],
      "saver",
    );
    const second = await verifyOriginal(
      page,
      await secondOriginal,
      fixtures[1],
    );
    await advancing(page);
    await expect
      .poll(() =>
        errors.media.some(
          (error) =>
            error.source === secondAttempt.source &&
            error.code === 4 &&
            error.trusted,
        ),
      )
      .toBe(true);

    const slider = seek(page);
    const bounds = await slider.boundingBox();
    expect(bounds).not.toBeNull();
    await slider.click({
      position: { x: bounds!.width / 2, y: bounds!.height / 2 },
    });
    await expect
      .poll(async () => (await mediaState(page)).position)
      .toBeGreaterThan(44);
    const forward = await advancing(page);
    expect(forward.absolute).toBeGreaterThan(44);
    expect(forward.absolute).toBeLessThan(53);
    expect(new URL(forward.source).pathname).toBe(fixtures[1].path);
    expect(new URL(forward.source).search).toBe("");
    await slider.click({
      position: { x: bounds!.width / 4, y: bounds!.height / 2 },
    });
    await expect
      .poll(async () => (await mediaState(page)).position)
      .toBeLessThan(25);
    const reverse = await advancing(page);
    expect(reverse.absolute).toBeGreaterThan(21);
    expect(reverse.absolute).toBeLessThan(30);

    await dock(page)
      .getByRole("button", { name: "Pause", exact: true })
      .click();
    await expect.poll(async () => (await mediaState(page)).paused).toBe(true);
    const paused = await mediaState(page);
    await page.waitForTimeout(700);
    expect(
      Math.abs((await mediaState(page)).position - paused.position),
    ).toBeLessThan(0.2);
    await dock(page).getByRole("button", { name: "Play", exact: true }).click();
    await advancing(page);

    const documentOrigin = await page.evaluate(() => performance.timeOrigin);
    const beforeNavigation = await mediaState(page);
    await page
      .getByRole("navigation", { name: "Main navigation" })
      .getByRole("link", { name: "Home", exact: true })
      .click();
    expect(await page.evaluate(() => performance.timeOrigin)).toBe(
      documentOrigin,
    );
    expect((await mediaState(page)).source).toBe(beforeNavigation.source);
    await advancing(page);

    await dock(page)
      .getByRole("button", { name: "Pause", exact: true })
      .click();
    await expect.poll(async () => (await mediaState(page)).paused).toBe(true);
    const checkpoint = Number(await seek(page).inputValue());
    await page.reload();
    await expect(dock(page)).toContainText("Fixture 660 Hz");
    await expect
      .poll(async () =>
        Math.abs(Number(await seek(page).inputValue()) - checkpoint),
      )
      .toBeLessThan(1.1);
    await expect(seek(page)).toHaveAttribute("max", "90");
    const restoredConverted = convertedResponse(
      page,
      "saver",
      (checkpoint - 1.1) * 1000,
      (checkpoint + 1.1) * 1000,
    );
    const restoredOriginal = originalResponse(page, fixtures[1].path);
    await dock(page).getByRole("button", { name: "Play", exact: true }).click();
    const restoredAttempt = await verifyUnsupportedConverted(
      page,
      await restoredConverted,
      fixtures[1],
      "saver",
    );
    const restored = await verifyOriginal(
      page,
      await restoredOriginal,
      fixtures[1],
    );
    const resumed = await advancing(page);
    expect(resumed.absolute).toBeGreaterThan(checkpoint - 1.1);
    expect(resumed.absolute).toBeLessThan(checkpoint + 8);
    await expect
      .poll(() =>
        errors.media.some(
          (error) =>
            error.source === restoredAttempt.source &&
            error.code === 4 &&
            error.trusted,
        ),
      )
      .toBe(true);

    await expect(await playbackSettings(page)).toHaveValue("saver");
    expect(
      await page.evaluate(() => localStorage.getItem("hito:stream-quality")),
    ).toBe("saver");
    await assertNoQualityDownloads(
      page,
      fixtures.map((fixture) => fixture.recording),
    );
    expect(errors.javascript).toEqual([]);
    expect(errors.unexpectedRequests).toEqual([]);
    const attempts = [attempted, secondAttempt, restoredAttempt];
    const attemptedUrls = attempts.map((attempt) => attempt.source);
    expect(errors.convertedMediaRequests).toEqual(attemptedUrls);
    expect(errors.media.map((error) => error.source).sort()).toEqual(
      [...attemptedUrls].sort(),
    );
    for (const error of errors.media) {
      expect(error.trusted).toBe(true);
      expect(error.code).toBe(4);
    }
    await testInfo.attach("chromium-original-fallback-evidence", {
      contentType: "application/json",
      body: JSON.stringify({
        attempts,
        convertedMediaRequests: errors.convertedMediaRequests,
        originals: [initial, second, restored],
        nativeErrors: errors.media,
      }),
    });
  },
);
