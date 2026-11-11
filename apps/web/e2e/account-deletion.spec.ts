import { expect, test } from "@playwright/test";

const password = "departing account test password";
const headers = { "x-hito-request": "1" };

test("an administrator deletes an account by typing its name and that account's session ends", async ({
  page,
  playwright,
}) => {
  const username = `departing-${Date.now() % 1000000}`;
  const created = await page.request.post("/api/v1/users", {
    headers,
    data: { username, password },
  });
  expect(created.status()).toBe(201);

  const baseURL = test.info().project.use.baseURL;
  const departing = await playwright.request.newContext({ baseURL });
  const login = await departing.post("/api/v1/login", {
    headers,
    data: { username, password },
  });
  expect(login.status()).toBe(200);
  expect((await departing.get("/api/v1/me")).status()).toBe(200);

  await page.goto("/settings?section=accounts");
  const accounts = page.getByRole("region", { name: "People" });
  const row = accounts
    .locator(".setting-line")
    .filter({ has: page.getByText(username, { exact: true }) });
  await expect(row).toBeVisible();

  await row.getByRole("button", { name: /^More for / }).click();
  await page.getByRole("menuitem", { name: "Delete account…" }).click();
  const dialog = page.getByRole("dialog", { name: "Delete account" });
  const confirm = dialog.getByRole("button", { name: "Delete account" });
  await expect(confirm).toBeDisabled();
  await dialog.getByLabel(`Type ${username} to confirm`).fill("someone else");
  await expect(confirm).toBeDisabled();
  await dialog.getByLabel(`Type ${username} to confirm`).fill(username);
  await expect(confirm).toBeEnabled();
  await confirm.click();

  await expect(dialog).toBeHidden();
  await expect(row).toHaveCount(0);
  await expect(page.getByText(`Deleted account ${username}`)).toBeVisible();
  expect((await departing.get("/api/v1/me")).status()).toBe(401);
  expect(
    (
      await page.request.delete("/api/v1/users/unknown-account", {
        headers,
        data: { username },
      })
    ).status(),
  ).toBe(404);
  await departing.dispose();
});
