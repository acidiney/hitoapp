import { expect, type Page } from "@playwright/test";
import { writeFile } from "node:fs/promises";
import { decoded, retained } from "./collection-artwork-actions";
import { openLibrarySection } from "./library-sections";

export async function retainedRetry(
  page: Page,
  route: string,
  path: string,
  expected: Buffer,
) {
  let failed = 0;
  let delivered = 0;
  await page.route(`**${path}`, async (intercept) => {
    if (intercept.request().resourceType() !== "image") {
      await intercept.continue();
      return;
    }
    if (!failed) {
      failed++;
      await intercept.fulfill({
        status: 503,
        headers: { "cache-control": "private, no-store" },
        body: "",
      });
    } else {
      delivered++;
      await intercept.continue();
    }
  });
  try {
    await page.goto(route);
    const hero = page.locator(".collection-hero .artwork");
    await expect(hero.locator("img")).toHaveCount(0);
    const retry = page.getByRole("button", {
      name: "Retry artwork",
      exact: true,
    });
    await expect(retry).toBeVisible();
    expect(failed).toBe(1);
    await retry.click();
    await decoded(hero.locator("img"), path);
    await expect(retry).toHaveCount(0);
    expect(delivered).toBe(1);
    await retained(page, path, expected);
    return { failed, delivered };
  } finally {
    await page.unroute(`**${path}`);
  }
}

export async function lateRendererRetry(
  page: Page,
  route: string,
  path: string,
  expected: Buffer,
) {
  let failed = 0;
  let delivered = 0;
  let heroFailed = false;
  let laterDecoded = false;
  let initialRowPending = false;
  let rowDistance = 0;
  await page.setViewportSize({ width: 1440, height: 200 });
  const stylesheet = await page.addStyleTag({
    content: ".album-hero { padding-bottom: 12000px !important; }",
  });
  const documentTime = await page.evaluate(() => performance.timeOrigin);
  try {
    await openLibrarySection(page, "Albums");
    expect(await page.evaluate(() => performance.timeOrigin)).toBe(
      documentTime,
    );
    const card = page
      .locator(".album-card")
      .filter({ has: page.locator(`a[href="${route}"]`) });
    await decoded(card.locator(".artwork img"), path);
    await page.route(`**${path}`, async (intercept) => {
      if (intercept.request().resourceType() !== "image") {
        await intercept.continue();
        return;
      }
      if (!failed) {
        failed++;
        await intercept.fulfill({
          status: 503,
          headers: { "cache-control": "private, no-store" },
          body: "",
        });
      } else {
        delivered++;
        await intercept.continue();
      }
    });
    await card.getByRole("heading", { level: 2 }).getByRole("link").click();
    await expect(page).toHaveURL(new RegExp(`${route}$`));
    expect(await page.evaluate(() => performance.timeOrigin)).toBe(
      documentTime,
    );
    const hero = page.locator(".album-hero .artwork");
    const row = page
      .getByRole("table", { name: "Tracks", exact: true })
      .locator(".artwork img")
      .first();
    await expect(hero.locator("img")).toHaveCount(0);
    heroFailed = true;
    const retry = page.getByRole("button", {
      name: "Retry artwork",
      exact: true,
    });
    await expect(retry).toBeVisible();
    expect(failed).toBe(1);
    expect(delivered).toBe(0);
    await expect(row).toHaveAttribute("src", path);
    initialRowPending = await row.evaluate(
      (element: HTMLImageElement) =>
        !element.complete && element.naturalWidth === 0,
    );
    expect(initialRowPending).toBe(true);
    rowDistance = await row.evaluate(
      (element) => element.getBoundingClientRect().top - innerHeight,
    );
    expect(rowDistance).toBeGreaterThan(10000);
    const received = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === path &&
        response.request().resourceType() === "image" &&
        response.status() === 200,
    );
    await row.scrollIntoViewIfNeeded();
    const image = await received;
    expect(await image.body()).toEqual(expected);
    await decoded(row, path);
    laterDecoded = true;
    expect(delivered).toBe(1);
    await expect(hero.locator("img")).toHaveCount(0);
    await expect(retry).toBeVisible();
    await retry.click();
    await hero.locator("img").scrollIntoViewIfNeeded();
    await decoded(hero.locator("img"), path);
    await expect(retry).toHaveCount(0);
    await retained(page, path, expected);
    expect(delivered).toBeLessThanOrEqual(3);
    return {
      control: "chromium-controlled-lazy-layout",
      distinctLaterSuccess: true,
      failed,
      delivered,
      heroFailed,
      laterDecoded,
      initialRowPending,
      rowDistance,
    };
  } finally {
    await writeFile(
      "/evidence/late-renderer-control.json",
      JSON.stringify({
        control: "chromium-controlled-lazy-layout",
        failed503: failed,
        subsequentImageRequests: delivered,
        heroFailed,
        laterDecoded,
        initialRowPending,
        rowDistance,
        rowImageEntities: await page
          .getByRole("table", { name: "Tracks", exact: true })
          .locator(".artwork img")
          .evaluateAll((images) =>
            images.map((element) =>
              (element as HTMLImageElement)
                .getAttribute("src")
                ?.includes("/releases/")
                ? "release"
                : "recording",
            ),
          ),
      }),
      { mode: 0o600 },
    );
    await page.unroute(`**${path}`);
    await stylesheet.evaluate((element) => {
      element.parentNode?.removeChild(element);
    });
    await page.setViewportSize({ width: 1440, height: 1000 });
  }
}

export async function coalescedRendererRetry(
  page: Page,
  route: string,
  path: string,
  fallback: string,
  expected: Buffer,
) {
  let failed = 0;
  let delivered = 0;
  await page.route(`**${path}`, async (intercept) => {
    if (intercept.request().resourceType() !== "image") {
      await intercept.continue();
      return;
    }
    if (!failed) {
      failed++;
      await intercept.fulfill({
        status: 503,
        headers: { "cache-control": "private, no-store" },
        body: "",
      });
    } else {
      delivered++;
      await intercept.continue();
    }
  });
  try {
    await page.goto(route);
    const hero = page.locator(".album-hero .artwork");
    const row = page
      .getByRole("table", { name: "Tracks", exact: true })
      .locator(".artwork img")
      .first();
    await expect(hero.locator("img")).toHaveCount(0);
    await decoded(row, fallback);
    expect(failed).toBe(1);
    expect(delivered).toBe(0);
    const retry = page.getByRole("button", {
      name: "Retry artwork",
      exact: true,
    });
    await expect(retry).toBeVisible();
    const received = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === path &&
        response.request().resourceType() === "image" &&
        response.status() === 200,
    );
    await retry.click();
    const image = await received;
    expect(await image.body()).toEqual(expected);
    await decoded(hero.locator("img"), path);
    await decoded(row, path);
    await expect(retry).toHaveCount(0);
    expect(delivered).toBeGreaterThanOrEqual(1);
    expect(delivered).toBeLessThanOrEqual(2);
    await retained(page, path, expected);
    const result = {
      control: "webkit-ordinary-coalesced-failure",
      distinctLaterSuccess: false,
      failed,
      delivered,
      heroInitiallyFailed: true,
      rowFallbackDecoded: true,
      bothReleaseDecoded: true,
    };
    await writeFile(
      "/evidence/coalesced-renderer-control.json",
      JSON.stringify(result),
      { mode: 0o600 },
    );
    return result;
  } finally {
    await page.unroute(`**${path}`);
  }
}
