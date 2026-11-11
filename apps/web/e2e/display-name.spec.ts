import { expect, test, type Page } from "@playwright/test";
import type { Schema } from "../src/shared/schema";

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
  browser,
  baseURL,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
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
        `/api/v1/libraries/${libraries.find((library: { personal?: boolean }) => !library.personal)!.id}/members/${member.id}`,
        { headers: headers(baseURL!) },
      )
    ).ok(),
  ).toBe(true);

  const context = await browser.newContext({
    baseURL,
    storageState: { cookies: [], origins: [] },
  });
  const memberPage = await context.newPage();
  memberPage.on("pageerror", (error) => errors.push(error.message));
  await signIn(memberPage, "display-name-member", memberPassword);
  const menu = memberPage.locator(".account-link");
  await expect(menu).toContainText("display-name-member");
  await memberPage.goto("/settings?section=profile");
  const field = memberPage.getByRole("textbox", { name: "Display name" });
  await expect(field).toHaveValue("");
  await expect(
    memberPage.getByRole("button", { name: "Save display name" }),
  ).toBeDisabled();

  await field.fill(hostile);
  await memberPage.getByRole("button", { name: "Save display name" }).click();
  await expect(memberPage.getByText("Display name saved.")).toBeVisible();
  await expect(menu).toContainText(hostile);
  await expect(menu).not.toContainText("display-name-member");
  await expect(menu.locator(".avatar")).toHaveText("IA");
  await expect(
    memberPage.locator("#setting-account-photo .setting-line-title"),
  ).toHaveText(hostile);
  await expect(
    memberPage.locator("#setting-account-photo .setting-line-hint"),
  ).toHaveText("display-name-member · Member");
  await expect(
    memberPage.locator(".account-link i, #setting-account-photo i"),
  ).toHaveCount(0);
  await memberPage.reload();
  await expect(
    memberPage.getByRole("textbox", { name: "Display name" }),
  ).toHaveValue(hostile);
  await expect(menu).toContainText(hostile);

  await field.fill("x".repeat(65));
  await memberPage.getByRole("button", { name: "Save display name" }).click();
  await expect(memberPage.getByRole("alert")).toContainText("64");

  await context.close();
  await page.goto("/settings?section=accounts");
  const row = page.locator(".setting-line", { hasText: "display-name-member" });
  await expect(row.locator(".setting-line-title")).toHaveText(hostile);
  await expect(row.locator(".setting-line-hint")).toContainText(
    "display-name-member · Member",
  );
  await expect(row.locator("i")).toHaveCount(0);

  expect(errors).toEqual([]);
});
