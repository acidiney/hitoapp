import { expect, test, type Page } from "@playwright/test";
import type { Schema } from "../src/shared/schema";

test.use({ storageState: { cookies: [], origins: [] } });

const adminPassword = "docker browser test password";
const memberPassword = "display name member password";
const hostile = "<i>Ana</i> & Co 🦉";

function headers(baseURL: string) {
  return { "x-hito-request": "1", origin: new URL(baseURL).origin };
}

async function signIn(page: Page, username: string, password: string) {
  await page.goto("/");
  await page.getByLabel("Username", { exact: true }).fill(username);
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
}

test("a member sets a display name that the menu, profile and the administrator's Accounts list show", async ({
  page,
  baseURL,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await signIn(page, "admin", adminPassword);
  const libraries: Schema<"Library">[] = await (
    await page.request.get("/api/v1/libraries")
  ).json();
  const created = await page.request.post("/api/v1/users", {
    headers: headers(baseURL!),
    data: { username: "display-name-member", password: memberPassword },
  });
  expect(created.status()).toBe(201);
  const member: Schema<"User"> = await created.json();
  expect(member.display_name ?? null).toBeNull();
  expect(
    (
      await page.request.put(
        `/api/v1/libraries/${libraries[0].id}/members/${member.id}`,
        { headers: headers(baseURL!) },
      )
    ).ok(),
  ).toBe(true);
  await page.request.post("/api/v1/logout", { headers: headers(baseURL!) });

  await signIn(page, "display-name-member", memberPassword);
  const menu = page.locator(".account-link");
  await expect(menu).toContainText("display-name-member");
  await page.goto("/settings?section=profile");
  const field = page.getByRole("textbox", { name: "Display name" });
  await expect(field).toHaveValue("");
  await expect(
    page.getByRole("button", { name: "Save display name" }),
  ).toBeDisabled();

  await field.fill(hostile);
  await page.getByRole("button", { name: "Save display name" }).click();
  await expect(page.getByText("Display name saved.")).toBeVisible();
  await expect(menu).toContainText(hostile);
  await expect(menu).not.toContainText("display-name-member");
  await expect(menu.locator(".avatar")).toHaveText("IA");
  await expect(
    page.locator(".profile-identity .setting-line-title"),
  ).toHaveText(hostile);
  await expect(page.locator(".profile-identity .setting-line-hint")).toHaveText(
    "display-name-member · Member",
  );
  await expect(
    page.locator(".account-link i, .profile-identity i"),
  ).toHaveCount(0);
  await page.reload();
  await expect(page.getByRole("textbox", { name: "Display name" })).toHaveValue(
    hostile,
  );
  await expect(menu).toContainText(hostile);

  await field.fill("x".repeat(65));
  await page.getByRole("button", { name: "Save display name" }).click();
  await expect(page.getByRole("alert")).toContainText("64");

  await page.request.post("/api/v1/logout", { headers: headers(baseURL!) });
  await signIn(page, "admin", adminPassword);
  await page.goto("/settings?section=accounts");
  const row = page.locator(".setting-line", { hasText: "display-name-member" });
  await expect(row.locator(".setting-line-title")).toHaveText(hostile);
  await expect(row.locator(".setting-line-hint")).toContainText(
    "display-name-member · Member",
  );
  await expect(row.locator("i")).toHaveCount(0);

  expect(errors).toEqual([]);
});
