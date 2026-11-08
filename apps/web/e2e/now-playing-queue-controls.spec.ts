import { expect, test, type Page } from "@playwright/test";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";

const first = "Fixture 440 Hz";
const second = "Fixture 660 Hz";

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

test("Change source in Now playing opens the Sources tab of the playing track", async ({
  page,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await playFixture(page, first);

  await dock(page)
    .getByRole("button", { name: "Open now playing", exact: true })
    .click();
  const player = page.getByRole("dialog", { name: "Now playing" });
  await player.getByRole("tab", { name: "Source" }).click();
  const card = player.getByRole("region", { name: "This source" });
  await expect(card.getByText("On this device")).toBeVisible();
  await expect(card).not.toContainText("This browser");
  await card.getByRole("button", { name: "Change source" }).click();

  await expect(page).toHaveURL(
    /\/recordings\/[^/?#]+\?tab=sources#find-audio$/,
  );
  await expect(page.getByRole("tab", { name: "Sources" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(page.getByRole("dialog", { name: "Now playing" })).toHaveCount(
    0,
  );
  expect(errors).toEqual([]);
});

test("Clear on the queue asks first, keeps the queue when cancelled and empties it when confirmed", async ({
  page,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await playFixture(page, second);

  await page.goto("/queue");
  await expect(
    page.getByRole("heading", { name: "Queue", level: 1 }),
  ).toBeVisible();
  const clear = page.getByRole("button", { name: "Clear", exact: true });
  await clear.click();
  const dialog = page.getByRole("dialog", { name: "Clear the queue?" });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(clear).toBeFocused();
  await expect(clear).toBeEnabled();

  await clear.click();
  await page
    .getByRole("dialog", { name: "Clear the queue?" })
    .getByRole("button", { name: "Clear queue" })
    .click();
  await expect(clear).toBeDisabled();
  await expect(page.getByText("Nothing is playing.")).toBeVisible();
  expect(errors).toEqual([]);
});
