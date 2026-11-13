import { expect, type Locator, type Page } from "@playwright/test";
import type { Browser } from "@playwright/test";
import { memberPassword } from "./member-accounts";
import { trackTitleLink } from "./track-row";

export const credentialMessage =
  "This music service connection needs its credentials updated.";
export const markLabel = "Credentials need updating";
export const accessLostMessage =
  "Your account access changed. Sign in again to use lyrics.";
export const settingsLinkName = "Open plugin settings";
export const trackTitle = "Fixture 440 Hz";

export const pluginsPath = "/settings?section=plugins";

export function lyricsPanel(page: Page): Locator {
  return page.getByRole("region", { name: "Lyrics", exact: true });
}

export function connectionCard(page: Page, label: string): Locator {
  return page
    .locator("li.plugin-connection")
    .filter({ has: page.getByRole("heading", { name: label, exact: true }) });
}

export async function playFromLibrary(page: Page, title = trackTitle) {
  await page.goto("/library?q=Hz");
  await page.getByRole("button", { name: "List view", exact: true }).click();
  await page
    .getByRole("table", { name: "Tracks", exact: true })
    .locator("tbody tr")
    .filter({ hasText: title })
    .getByRole("button", { name: `Play ${title}`, exact: true })
    .click();
  await expect.poll(() => audioTime(page)).toBeGreaterThan(0.2);
}

export function audioTime(page: Page): Promise<number> {
  return page
    .locator("audio[data-hito-player]")
    .evaluate((audio: HTMLAudioElement) => audio.currentTime);
}

export async function openLyricsFromLibrary(page: Page, title = trackTitle) {
  await trackTitleLink(page.locator("main"), title).first().click();
  return ensureLyricsTab(page);
}

export async function ensureLyricsTab(page: Page) {
  const tab = page.getByRole("tab", { name: "Lyrics" });
  await expect(tab).toBeVisible();
  if ((await tab.getAttribute("aria-selected")) !== "true") await tab.click();
  return lyricsPanel(page);
}

export async function showLyrics(panel: Locator) {
  const show = panel.getByRole("button", { name: "Show lyrics" });
  const failed = panel.getByRole("alert");
  const shown = panel.getByText("Plain river line");
  await expect(show.or(failed).or(shown).first()).toBeVisible();
  if (await show.isVisible()) await show.click();
}

export async function expectAudioPlaying(page: Page) {
  const audio = page.locator("audio[data-hito-player]");
  expect(await audio.evaluate((item: HTMLAudioElement) => !item.paused)).toBe(
    true,
  );
  const before = await audioTime(page);
  await expect.poll(() => audioTime(page)).toBeGreaterThan(before);
}

export async function expectQueueHolds(page: Page, title = trackTitle) {
  const toggle = page
    .getByRole("contentinfo", { name: "Audio player", exact: true })
    .getByRole("button", { name: /^Open queue/ });
  await toggle.click();
  const queue = page.locator("#queue-panel");
  await expect(
    queue.getByRole("region", { name: "Now playing" }),
  ).toContainText(title);
  await queue.getByRole("button", { name: "Close queue" }).click();
  await expect(queue).toBeHidden();
}

export async function networkQuiet(
  page: Page,
  quietMs = 1_500,
  limitMs = 15_000,
  matcher = /\/api\/v1\/(recordings\/[^/]+\/lyrics|plugins)/,
) {
  let last = Date.now();
  const touch = (request: { url(): string }) => {
    if (matcher.test(new URL(request.url()).pathname)) last = Date.now();
  };
  page.on("request", touch);
  page.on("requestfinished", touch);
  page.on("requestfailed", touch);
  try {
    await expect
      .poll(() => Date.now() - last, {
        timeout: limitMs,
        intervals: [200],
      })
      .toBeGreaterThanOrEqual(quietMs);
  } finally {
    page.off("request", touch);
    page.off("requestfinished", touch);
    page.off("requestfailed", touch);
  }
}

export async function memberSession(
  browser: Browser,
  baseURL: string,
  username: string,
) {
  const context = await browser.newContext({
    baseURL,
    storageState: { cookies: [], origins: [] },
  });
  const login = await context.request.post("/api/v1/login", {
    headers: { "x-hito-request": "1", origin: new URL(baseURL).origin },
    data: { username, password: memberPassword },
  });
  expect(login.status()).toBe(200);
  return context;
}

export async function submitSignIn(page: Page, username: string) {
  await expect(
    page.getByRole("heading", { name: "Sign in", exact: true }),
  ).toBeVisible();
  await page.getByLabel("Username", { exact: true }).fill(username);
  await page.getByLabel("Password", { exact: true }).fill(memberPassword);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
}

export async function openSettingsPlugins(page: Page) {
  await page
    .getByRole("navigation", { name: "Main navigation" })
    .getByRole("link", { name: "Plugins", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Plugins", exact: true }),
  ).toBeVisible();
}

export async function chooseService(scope: Locator, label: string) {
  const pill = scope
    .getByRole("group", { name: "Music service" })
    .getByRole("button", { name: label, exact: true });
  if ((await pill.count()) > 0) {
    await pill.click();
    return;
  }
  await scope
    .getByRole("combobox", { name: "Music service", exact: true })
    .click();
  await scope.getByRole("option", { name: label, exact: true }).click();
}
