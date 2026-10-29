import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, type Page } from "@playwright/test";

export const adminCredentials = {
  username: "admin",
  password: "docker browser test password",
};
export const adminSessionPath = join(tmpdir(), "hito-e2e-admin-session.json");

export const sharedAdminSession = !Object.entries(process.env).some(
  ([name, value]) => /^HITO_.+_E2E$/.test(name) && value === "1",
);

/** Creates the administrator through the first-run screen, or signs in when it already exists. */
export async function enterAdmin(page: Page) {
  await page.goto("/");
  const setup = page.getByRole("heading", { name: "Set up Hito" });
  const welcome = page.getByRole("heading", { name: "Sign in", exact: true });
  await expect(setup.or(welcome)).toBeVisible();
  const firstRun = await setup.isVisible();
  await page
    .getByLabel("Username", { exact: true })
    .fill(adminCredentials.username);
  await page
    .getByLabel("Password", { exact: true })
    .fill(adminCredentials.password);
  await page
    .getByRole("button", {
      name: firstRun ? "Create administrator" : "Sign in",
      exact: true,
    })
    .click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
}
