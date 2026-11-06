import { expect, test, type Page, type Route } from "@playwright/test";
import { openLibrarySection } from "./fixtures/library-sections";

type Outage = "closed" | "gateway" | "health-only" | "up";

async function simulate(page: Page) {
  const state: { mode: Outage } = { mode: "up" };
  await page.route("**/*", (route: Route) => {
    const request = route.request();
    const url = new URL(request.url());
    const talksToHito =
      url.pathname === "/health" ||
      url.pathname.startsWith("/api/v1/") ||
      request.headers()["x-inertia"] !== undefined;
    if (!talksToHito || state.mode === "up") return route.continue();
    if (state.mode === "health-only" && url.pathname === "/health") {
      state.mode = "up";
      return route.continue();
    }
    if (state.mode === "closed" || state.mode === "health-only")
      return route.abort("connectionrefused");
    return route.fulfill({
      status: 502,
      contentType: "text/html",
      body: "<html><body>502 Bad Gateway nginx 10.0.0.7</body></html>",
    });
  });
  return state;
}

for (const mode of ["closed", "gateway"] as const) {
  test(`a ${mode} server is explained in plain words and the page recovers in place`, async ({
    page,
  }) => {
    test.setTimeout(90_000);
    const errors: string[] = [];
    page.on("pageerror", (error) => errors.push(error.message));
    const state = await simulate(page);
    await page.goto("/library");
    const heading = page.getByRole("heading", {
      name: "Your library",
      exact: true,
    });
    await expect(heading).toBeVisible();
    const search = page.getByRole("textbox", {
      name: "Search library",
      exact: true,
    });
    await search.fill("kept across the outage");

    state.mode = mode;
    await openLibrarySection(page, "Albums");
    const notice = page.getByRole("status").filter({ hasText: "Cannot reach" });
    await expect(notice).toBeVisible();
    const text = (await notice.textContent()) ?? "";
    expect(text).toContain("Reconnecting automatically");
    expect(text).not.toMatch(/502|Bad Gateway|nginx|10\.0\.0\.7|fetch|\/api/i);
    await expect(page.locator("iframe")).toHaveCount(0);
    await expect(page.locator("dialog[open]")).toHaveCount(0);
    await expect(heading).toBeVisible();
    await expect(page).toHaveURL(/\/library$/);
    await expect(search).toHaveValue("kept across the outage");

    await page.waitForTimeout(2500);
    await expect(notice).toBeVisible();

    state.mode = "up";
    await expect(notice).toBeHidden({ timeout: 20_000 });
    await expect(heading).toBeVisible();
    await expect(search).toHaveValue("kept across the outage");

    await openLibrarySection(page, "Albums");
    await expect(
      page.getByRole("heading", { name: "Albums", exact: true }),
    ).toBeVisible();
    expect(errors).toEqual([]);
  });
}

test("Try now reconnects without waiting for the next automatic attempt", async ({
  page,
}) => {
  const state = await simulate(page);
  await page.clock.install();
  await page.goto("/library");
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
  await page.clock.pauseAt(new Date(Date.now() + 3_600_000));
  state.mode = "closed";
  await openLibrarySection(page, "Albums");
  const notice = page.getByRole("status").filter({ hasText: "Cannot reach" });
  await expect(notice).toBeVisible();
  state.mode = "health-only";
  await notice.getByRole("button", { name: "Try now" }).click();
  await expect(notice).toBeHidden({ timeout: 3000 });
});
