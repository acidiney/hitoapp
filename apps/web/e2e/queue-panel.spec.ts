import { expect, test, type Page } from "@playwright/test";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";

const first = "Fixture 440 Hz";

function dock(page: Page) {
  return page.getByRole("contentinfo", { name: "Audio player", exact: true });
}

async function playFixture(page: Page, title: string) {
  await page.goto("/library?q=Hz");
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "List view", exact: true }).click();
  await page
    .getByRole("table", { name: "Tracks", exact: true })
    .locator("tbody tr")
    .filter({ hasText: title })
    .getByRole("button", { name: `Play ${title}`, exact: true })
    .click();
  await expect
    .poll(() =>
      page
        .locator("audio[data-hito-player]")
        .evaluate((audio: HTMLAudioElement) => audio.currentTime),
    )
    .toBeGreaterThan(0.2);
}

test("the dock Queue button docks the queue beside the page, which keeps working, and Escape closes it", async ({
  page,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await playFixture(page, first);

  const panel = page.locator("#queue-panel");
  const toggle = dock(page).getByRole("button", { name: /^Open queue/ });
  await expect(toggle).toHaveAttribute("aria-expanded", "false");
  await expect(toggle).toHaveAttribute("aria-controls", "queue-panel");
  await expect(panel).toBeHidden();
  const wide = (await page.locator(".workspace").boundingBox())!.width;

  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-expanded", "true");
  await expect(panel).toBeVisible();
  await expect(
    panel.getByRole("heading", { name: "Queue", level: 2 }),
  ).toBeFocused();
  await expect(
    panel.getByRole("region", { name: "Now playing" }),
  ).toBeVisible();
  await expect
    .poll(async () => (await page.locator(".workspace").boundingBox())!.width)
    .toBeLessThan(wide - 200);
  const sidebar = (await page.locator(".sidebar").boundingBox())!;
  const box = (await panel.boundingBox())!;
  expect(box.x).toBeGreaterThan(sidebar.x + sidebar.width);
  expect(box.x + box.width).toBeCloseTo(page.viewportSize()!.width, 0);

  await page.getByRole("link", { name: "Home", exact: true }).first().click();
  await expect(page).toHaveURL(/\/home$/);
  await expect(panel).toBeVisible();
  await page.reload();
  await expect(panel).toBeVisible();

  await panel.getByRole("heading", { name: "Queue", level: 2 }).focus();
  await page.keyboard.press("Escape");
  await expect(panel).toBeHidden();
  await expect(toggle).toBeFocused();
  await expect(toggle).toHaveAttribute("aria-expanded", "false");
  await expect
    .poll(async () => (await page.locator(".workspace").boundingBox())!.width)
    .toBeCloseTo(wide, 0);

  await page.keyboard.press("q");
  await expect(panel).toBeVisible();
  await panel.getByRole("button", { name: "Close queue" }).click();
  await expect(panel).toBeHidden();
  expect(errors).toEqual([]);
});

test("the /queue link opens the docked panel over the page it came from", async ({
  page,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  await playFixture(page, first);
  await page.goto("/queue");
  await expect(page.locator("#queue-panel[data-open]")).toBeVisible();
  await expect(page).not.toHaveURL(/\/queue$/);
});
