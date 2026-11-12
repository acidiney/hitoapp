import {
  accountOptionsButton,
  lockFromShell,
  openAccountOptions,
} from "./fixtures/account";
import { expect, test, type Page } from "@playwright/test";
import { adminCredentials } from "./fixtures/admin-session";

const pin = "246810";

async function removePin(page: Page) {
  await page.goto("/settings?section=privacy");
  const remove = page.getByRole("button", { name: "Remove", exact: true });
  if (!(await remove.isVisible())) return;
  await remove.click();
  const dialog = page.getByRole("dialog", { name: "Remove the PIN?" });
  await dialog
    .getByLabel("Your password", { exact: true })
    .fill(adminCredentials.password);
  await dialog.getByRole("button", { name: "Remove PIN" }).click();
  await expect(page.getByRole("button", { name: "Set a PIN" })).toBeVisible();
}

test("locking shows a lock screen in every tab and the PIN restores the same session", async ({
  page,
  context,
}) => {
  try {
    await page.goto("/settings?section=privacy");
    const options = await openAccountOptions(page);
    await expect(
      options.getByRole("menuitem", { name: "Sign out", exact: true }),
    ).toBeVisible();
    await expect(
      options.getByRole("menuitem", { name: "Lock", exact: true }),
    ).toHaveCount(0);
    await page.keyboard.press("Escape");
    await expect(accountOptionsButton(page)).toBeFocused();
    await page.getByRole("button", { name: "Set a PIN" }).click();
    const pinDialog = page.getByRole("dialog", { name: "Set a PIN" });
    await pinDialog.getByLabel("PIN", { exact: true }).fill(pin);
    await pinDialog
      .getByLabel("Your password", { exact: true })
      .fill(adminCredentials.password);
    await pinDialog.getByRole("button", { name: "Save PIN" }).click();
    await expect(
      page.getByRole("button", { name: "Change PIN" }),
    ).toBeVisible();

    const other = await context.newPage();
    await other.goto("/library");
    await expect(
      other.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();

    await lockFromShell(page);
    const lockScreen = page.getByRole("heading", { name: "Locked" });
    await expect(lockScreen).toBeVisible();
    await expect(page.getByText(adminCredentials.username)).toBeVisible();
    await expect(
      page.getByText("Music stopped when this locked"),
    ).toBeVisible();
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
