import { expect, test, type Page } from "@playwright/test";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";

test.use({ deviceScaleFactor: 1, viewport: { width: 1440, height: 900 } });

const title = "Fixture 440 Hz";

async function shot(page: Page, name: string) {
  await page.waitForTimeout(400);
  await page.screenshot({ path: test.info().outputPath(`${name}.png`) });
}

test("Now playing changes from No source to Ready when a source is resolved after playback starts", async ({
  page,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  await page.emulateMedia({ colorScheme: "dark" });
  let resolved = false;
  await page.route("**/api/v1/recordings/*/playback-sources", async (route) => {
    const id = route.request().url().split("/recordings/")[1].split("/")[0];
    await route.fulfill({
      json: resolved
        ? [
            {
              id: "00000000-0000-4000-8000-000000000001",
              recording_id: id,
              provider: "youtube",
              provider_label: "YouTube Music",
              label: "Opus 128 kbps",
              match_kind: "verified",
              ready: true,
              can_prepare: true,
            },
          ]
        : [],
    });
  });
  await page.goto("/library?q=Hz");
  await page.getByRole("button", { name: "List view", exact: true }).click();
  await page
    .getByRole("table", { name: "Tracks", exact: true })
    .locator("tbody tr")
    .filter({ hasText: title })
    .getByRole("button", { name: `Play ${title}`, exact: true })
    .click();
  await page
    .getByRole("contentinfo", { name: "Audio player", exact: true })
    .getByRole("button", { name: "Open now playing", exact: true })
    .click();
  const player = page.getByRole("dialog", { name: "Now playing" });
  const card = player.getByRole("region", { name: "This source" });
  await expect(
    card.getByText("This track has no playable source yet."),
  ).toBeVisible();
  await expect(player.getByText("No source").first()).toBeVisible();
  await shot(page, "before");

  resolved = true;
  await player.getByRole("button", { name: /^(Pause|Play)$/ }).first().click();
  await player.getByRole("button", { name: /^(Pause|Play)$/ }).first().click();
  await expect(card.getByText("YouTube Music")).toBeVisible();
  await expect(card.getByText("Ready")).toBeVisible();
  await expect(player.getByText("from YouTube Music").first()).toBeVisible();
  await expect(player.getByText("No source")).toHaveCount(0);
  await shot(page, "after");
});
