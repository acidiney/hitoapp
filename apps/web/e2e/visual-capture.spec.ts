import { expect, test, type Page } from "@playwright/test";
import { openLibrarySection } from "./fixtures/library-sections";

const capture = process.env.HITO_VISUAL_CAPTURE === "1";
const outputFolder = "/workspace/test-results/visual";
const sizes = [
  { name: "1440", width: 1440, height: 900 },
  { name: "390", width: 390, height: 844 },
] as const;
const settingsSections = [
  "profile",
  "privacy",
  "music",
  "plugins",
  "playback",
  "downloads",
  "accounts",
  "updates",
  "about",
];

test.skip(!capture, "Set HITO_VISUAL_CAPTURE=1 to capture the screenshots");

async function shoot(page: Page, name: string, size: string) {
  await page.waitForTimeout(500);
  await page.screenshot({ path: `${outputFolder}/${name}-${size}.png` });
  const viewport = page.viewportSize()!;
  const height = await page.evaluate(
    () => document.documentElement.scrollHeight,
  );
  if (height > viewport.height) {
    await page.setViewportSize({ width: viewport.width, height });
    await page.waitForTimeout(300);
    await page.screenshot({ path: `${outputFolder}/${name}-${size}-full.png` });
    await page.setViewportSize(viewport);
  }
}

async function seedLibrary(page: Page) {
  await page.goto("/settings?section=music");
  for (const folder of ["Music", "Archive"]) {
    await page.getByRole("button", { name: "Add music folder" }).click();
    const picker = page.getByRole("dialog", { name: "Add a music folder" });
    await picker
      .getByRole("button", { name: `Open ${folder}`, exact: true })
      .click();
    await picker.getByRole("button", { name: "Use this folder" }).click();
    await expect(picker).not.toBeVisible();
  }
  await expect(
    page
      .getByRole("list", { name: "Connected music folders" })
      .getByRole("listitem")
      .filter({ hasText: /\d+ files? read/ }),
  ).toHaveCount(2, { timeout: 30000 });
  await page.goto("/library");
  const play = page.getByRole("button", { name: /^Play / }).first();
  await expect(play).toBeEnabled({ timeout: 20000 });
  await play.click();
  await expect
    .poll(() =>
      page
        .locator("audio[data-hito-player]")
        .evaluate((audio: HTMLAudioElement) => audio.currentTime),
    )
    .toBeGreaterThan(3);
  await page.getByRole("button", { name: "Pause", exact: true }).click();
  await page.waitForTimeout(1500);
  await page.getByRole("button", { name: "Play", exact: true }).click();
  await openLibrarySection(page, "Playlists");
  await page.getByLabel("New playlist", { exact: true }).fill("Late night");
  await page.getByRole("button", { name: "Create playlist" }).click();
  await expect(
    page.locator(".playlist-list").getByText("Late night"),
  ).toBeVisible();
}

async function fillPlaylist(page: Page) {
  await page.goto("/library");
  const adders = page.getByLabel(/^Add .* to playlist$/);
  await expect(adders.first()).toBeVisible();
  for (let index = 0; index < 4; index += 1) {
    await adders.nth(index).selectOption({ label: "Late night" });
    await page.waitForTimeout(400);
  }
  for (let index = 0; index < 3; index += 1) {
    await page
      .getByRole("button", { name: /^Like / })
      .first()
      .click();
    await page.waitForTimeout(400);
  }
}

async function captureCollections(page: Page) {
  await page.setViewportSize({ width: 1440, height: 900 });
  await fillPlaylist(page);
  for (const size of sizes) {
    await page.setViewportSize({ width: size.width, height: size.height });
    await page.goto("/library");
    await shoot(page, "library-tracks", size.name);
    await page.getByRole("button", { name: "List view" }).click();
    await shoot(page, "library-tracks-list", size.name);
    await page.goto("/albums");
    await shoot(page, "albums", size.name);
    await page.goto("/artists");
    await shoot(page, "artists", size.name);
    await page.goto("/playlists");
    await shoot(page, "playlists", size.name);
    await page.goto("/likes");
    await shoot(page, "likes", size.name);
    await page.goto("/history");
    await shoot(page, "history", size.name);
    await page.goto("/artists");
    await page
      .locator(".artist-grid a")
      .filter({ hasText: "Lume Areia" })
      .first()
      .click();
    await expect(page).toHaveURL(/\/artists\//);
    await shoot(page, "artist-credit", size.name);
    await page.goto("/albums");
    await page.locator(".album-grid a").first().click();
    await expect(page).toHaveURL(/\/albums\//);
    await shoot(page, "album", size.name);
    await page.goto("/playlists");
    await page
      .getByRole("link", { name: /Late night/ })
      .first()
      .click();
    await expect(page).toHaveURL(/\/playlists\//);
    await shoot(page, "playlist", size.name);
    const edit = page.getByRole("button", { name: "Edit", exact: true });
    if (await edit.isVisible()) {
      await edit.click();
      await shoot(page, "playlist-edit", size.name);
    }
  }
}

test("capture every redesigned surface at desktop and phone width", async ({
  page,
  browser,
}) => {
  test.setTimeout(420000);
  await page.setViewportSize({ width: 1440, height: 900 });
  await seedLibrary(page);
  const tabs = page.getByRole("navigation", { name: "Main navigation" });
  for (const size of sizes) {
    await page.setViewportSize({ width: size.width, height: size.height });
    await tabs.getByRole("link", { name: "Home", exact: true }).click();
    await expect(page).toHaveURL(/\/home$/);
    await shoot(page, "home", size.name);
    await page
      .getByRole("link", { name: "Discover music from your services" })
      .click();
    await expect(page).toHaveURL(/\/discover/);
    await shoot(page, "discover", size.name);
    await tabs.getByRole("link", { name: /^Search/ }).click();
    await expect(page).toHaveURL(/\/search$/);
    await shoot(page, "search", size.name);
    await page.getByRole("searchbox").first().fill("Lume");
    await page.keyboard.press("Enter");
    await page.waitForTimeout(800);
    await shoot(page, "search-results", size.name);
    await tabs.getByRole("link", { name: "Library", exact: true }).click();
    await expect(page).toHaveURL(/\/library$/);
    await shoot(page, "library", size.name);
    await page
      .getByRole("link", { name: "Account and settings", exact: true })
      .click();
    await expect(page).toHaveURL(/\/settings/);
    await shoot(page, "settings-index", size.name);
    const nav = page.getByRole("navigation", { name: "Settings sections" });
    for (const section of settingsSections) {
      if (!(await nav.isVisible()))
        await page.getByRole("link", { name: "All settings" }).click();
      await nav.locator(`a[href$="section=${section}"]`).click();
      await expect(page).toHaveURL(new RegExp(`section=${section}$`));
      await shoot(page, `settings-${section}`, size.name);
    }
  }
  await captureCollections(page);
  const guest = await browser.newContext({
    storageState: { cookies: [], origins: [] },
  });
  const login = await guest.newPage();
  for (const size of sizes) {
    await login.setViewportSize({ width: size.width, height: size.height });
    await login.goto("/");
    await expect(
      login.getByRole("heading", { name: "Sign in", exact: true }),
    ).toBeVisible();
    await shoot(login, "sign-in", size.name);
  }
  await guest.close();
});
