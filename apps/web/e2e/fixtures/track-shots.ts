import type { Page } from "@playwright/test";

type View = "desktop-dark" | "desktop-light" | "phone-dark";

const sizes: Record<
  View,
  { width: number; height: number; scheme: "dark" | "light" }
> = {
  "desktop-dark": { width: 1280, height: 800, scheme: "dark" },
  "desktop-light": { width: 1280, height: 800, scheme: "light" },
  "phone-dark": { width: 375, height: 812, scheme: "dark" },
};

export async function trackShot(
  page: Page,
  name: string,
  view: View = "desktop-dark",
  full = false,
) {
  const directory = process.env.TRACK_SHOTS;
  if (!directory) return;
  const original = page.viewportSize();
  const size = sizes[view];
  await page.emulateMedia({ colorScheme: size.scheme });
  await page.setViewportSize({ width: size.width, height: size.height });
  await page.waitForTimeout(400);
  await page.screenshot({
    path: `${directory}/${name}-${view}.png`,
    fullPage: full,
  });
  await page.emulateMedia({ colorScheme: null });
  if (original) await page.setViewportSize(original);
}
