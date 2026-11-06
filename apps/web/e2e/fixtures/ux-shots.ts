import type { Locator, Page } from "@playwright/test";

const views = [
  { label: "desktop-dark", scheme: "dark", width: 1280, height: 800 },
  { label: "desktop-light", scheme: "light", width: 1280, height: 800 },
  { label: "phone-dark", scheme: "dark", width: 375, height: 812 },
] as const;

export async function shoot(
  page: Page,
  name: string,
  options: { focus?: Locator; full?: boolean } = {},
) {
  const directory = process.env.UX_SHOTS;
  if (!directory) return;
  const original = page.viewportSize();
  for (const view of views) {
    await page.emulateMedia({ colorScheme: view.scheme });
    await page.setViewportSize({ width: view.width, height: view.height });
    await options.focus?.scrollIntoViewIfNeeded();
    await page.waitForTimeout(300);
    await page.screenshot({
      path: `${directory}/${name}-${view.label}.png`,
      fullPage: options.full ?? false,
    });
  }
  await page.emulateMedia({ colorScheme: null });
  if (original) await page.setViewportSize(original);
}
