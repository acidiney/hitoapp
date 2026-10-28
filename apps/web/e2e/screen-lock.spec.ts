import { expect, test, type Page } from "@playwright/test";
import { adminCredentials } from "./fixtures/admin-session";

const pin = "246810";

async function removePin(page: Page) {
  await page.goto("/settings?section=privacy");
  const remove = page.getByRole("button", { name: "Remove PIN" });
  if (!(await remove.isVisible())) return;
  await page
    .getByLabel("Current password", { exact: true })
    .fill(adminCredentials.password);
  await remove.click();
  await expect(page.getByRole("button", { name: "Save PIN" })).toBeVisible();
}

test("locking shows a lock screen in every tab and the PIN restores the same session", async ({
  page,
  context,
}) => {
  try {
    await page.goto("/settings?section=privacy");
    await expect(
      page.getByRole("button", { name: "Lock", exact: true }),
    ).toHaveCount(0);
    await page.getByLabel("PIN", { exact: true }).fill(pin);
    await page
      .getByLabel("Current password", { exact: true })
      .fill(adminCredentials.password);
    await page.getByRole("button", { name: "Save PIN" }).click();
    await expect(
      page.getByRole("button", { name: "Remove PIN" }),
    ).toBeVisible();

    const other = await context.newPage();
    await other.goto("/library");
    await expect(
      other.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();

    await page.getByRole("button", { name: "Lock", exact: true }).click();
    const lockScreen = page.getByRole("heading", { name: "Locked" });
    await expect(lockScreen).toBeVisible();
    await expect(page.getByText(adminCredentials.username)).toBeVisible();
    await expect(
      page.getByRole("navigation", { name: "Main navigation" }),
    ).toHaveCount(0);

    await other.reload();
    await expect(other.getByRole("heading", { name: "Locked" })).toBeVisible();
    const refused = await other.request.get("/api/v1/libraries");
    expect(refused.status()).toBe(423);
    expect((await refused.json()).code).toBe("locked");

    await page.getByLabel("PIN", { exact: true }).fill("000000");
    await page.getByRole("button", { name: "Unlock" }).click();
    await expect(page.getByRole("alert")).toContainText("not correct");
    await expect(lockScreen).toBeVisible();

    await page.getByLabel("PIN", { exact: true }).fill(pin);
    await page.getByRole("button", { name: "Unlock" }).click();
    await expect(
      page.getByRole("navigation", { name: "Main navigation" }),
    ).toBeVisible();
    await other.reload();
    await expect(
      other.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
  } finally {
    await removePin(page);
  }
});
