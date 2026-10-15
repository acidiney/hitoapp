import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { expect, test, type Page } from "@playwright/test";

async function signIn(page: Page, username: string, password: string) {
  await page.goto("/settings");
  await page.getByLabel("Username", { exact: true }).fill(username);
  await page.getByLabel("Password", { exact: true }).fill(password);
  const setup = await page
    .getByRole("button", { name: "Create administrator" })
    .count();
  await page
    .getByRole("button", {
      name: setup ? "Create administrator" : "Sign in",
      exact: true,
    })
    .click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
  await page.goto("/settings");
  await page.getByRole("tab", { name: "Plugins", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Your plugins", exact: true }),
  ).toBeVisible();
}

test("personal plugin connections review changes and remain private across accounts", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  let manifest = {
    id: "org.hito.browser.fixture",
    name: "Fixture collection",
    protocol_version: 1,
    capabilities: ["search", "sources"],
  };
  let expectedToken: string | null = "browser-private-plugin-token";
  const requests: (string | undefined)[] = [];
  const service = createServer((request, response) => {
    requests.push(request.headers.authorization);
    if (
      request.url !== "/fixture/v1/manifest" ||
      request.headers.authorization !==
        (expectedToken ? `Bearer ${expectedToken}` : undefined)
    ) {
      response.writeHead(401).end("private fixture rejection");
      return;
    }
    response
      .writeHead(200, { "content-type": "application/json" })
      .end(JSON.stringify(manifest));
  });
  await new Promise<void>((resolve) => service.listen(0, "127.0.0.1", resolve));
  const address = `http://127.0.0.1:${(service.address() as AddressInfo).port}/fixture/`;
  try {
    await signIn(page, "admin", "docker browser test password");
    await page.getByText("Services for this server", { exact: true }).click();
    await page
      .getByLabel("Service name", { exact: true })
      .fill("Browser fixture service");
    await page.getByLabel("Service address", { exact: true }).fill(address);
    await page
      .getByRole("button", { name: "Offer service", exact: true })
      .click();
    await expect(
      page.getByRole("combobox", { name: "Service", exact: true }),
    ).toHaveValue(/.+/);
    await page.setViewportSize({ width: 390, height: 844 });
    const connectButton = page.getByRole("button", {
      name: "Connect service",
      exact: true,
    });
    await connectButton.scrollIntoViewIfNeeded();
    const connectBounds = await connectButton.boundingBox();
    const dockBounds = await page
      .getByRole("contentinfo", { name: "Audio player" })
      .boundingBox();
    expect(connectBounds!.y + connectBounds!.height).toBeLessThanOrEqual(
      dockBounds!.y,
    );
    const selector = await page
      .getByRole("combobox", { name: "Service", exact: true })
      .boundingBox();
    expect(selector!.height).toBeGreaterThanOrEqual(44);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      ),
    ).toBe(false);
    await page.screenshot({
      path: "/workspace/test-results/plugins-connect-mobile.png",
    });
    const serviceAddress = page.getByLabel("Service address", { exact: true });
    await serviceAddress.evaluate((element) =>
      element.scrollIntoView({ block: "center" }),
    );
    const addressBounds = await serviceAddress.boundingBox();
    expect(addressBounds!.y + addressBounds!.height).toBeLessThanOrEqual(
      dockBounds!.y,
    );
    await page.screenshot({
      path: "/workspace/test-results/plugins-admin-mobile.png",
    });
    await page.setViewportSize({ width: 1440, height: 1000 });
    await page.evaluate(() => window.scrollTo(0, 0));
    await page
      .getByLabel("Bearer token (optional)", { exact: true })
      .fill(expectedToken!);
    await page
      .getByRole("button", { name: "Connect service", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Fixture collection", exact: true }),
    ).toBeVisible();
    expect(requests[0]).toBe(`Bearer ${expectedToken}`);
    manifest = {
      ...manifest,
      name: "Fixture collection updated",
      capabilities: ["metadata", "sources"],
    };
    await page
      .getByRole("button", { name: "Check connection", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Review plugin changes", exact: true }),
    ).toBeVisible();
    await page.screenshot({
      path: "/workspace/test-results/plugins-review-desktop.png",
    });
    await page.setViewportSize({ width: 390, height: 844 });
    await page
      .getByRole("button", { name: "Accept changes", exact: true })
      .scrollIntoViewIfNeeded();
    await page.screenshot({
      path: "/workspace/test-results/plugins-review-mobile.png",
    });
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      ),
    ).toBe(false);
    await page
      .getByRole("button", { name: "Accept changes", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Review plugin changes", exact: true }),
    ).toHaveCount(0);
    await expect(
      page.getByRole("heading", {
        name: "Fixture collection updated",
        exact: true,
      }),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Check connection", exact: true }),
    ).toBeFocused();
    await page.setViewportSize({ width: 1440, height: 1000 });
    await page.getByText("Connection settings", { exact: true }).click();
    await page
      .getByLabel("New bearer token", { exact: true })
      .fill("wrong-token");
    await page.getByRole("button", { name: "Save token", exact: true }).click();
    await expect(page.getByRole("alert")).toContainText("rejected");
    await expect(
      page.getByLabel("New bearer token", { exact: true }),
    ).toHaveValue("wrong-token");
    expectedToken = "replacement-browser-token";
    await page
      .getByLabel("New bearer token", { exact: true })
      .fill(expectedToken);
    await page.getByRole("button", { name: "Save token", exact: true }).click();
    await expect(
      page.getByLabel("New bearer token", { exact: true }),
    ).toHaveValue("");
    await expect(
      page.getByText("Connection checked.", { exact: true }),
    ).toBeVisible();
    const beforeClear = requests.length;
    await page
      .getByRole("button", { name: "Clear saved token", exact: true })
      .click();
    await expect(
      page.getByText("No token saved", { exact: true }),
    ).toBeVisible();
    expect(requests.length).toBe(beforeClear);
    await expect(
      page.getByRole("button", { name: "Check connection", exact: true }),
    ).toBeFocused();
    expectedToken = null;
    await page
      .getByRole("button", { name: "Check connection", exact: true })
      .click();
    await expect.poll(() => requests.length).toBe(beforeClear + 1);
    await expect(
      page.getByRole("button", { name: "Check connection", exact: true }),
    ).toBeEnabled();
    await page.screenshot({
      path: "/workspace/test-results/plugins-connected-desktop.png",
    });
    await page.getByRole("tab", { name: "Accounts", exact: true }).click();
    await page.getByLabel("Username", { exact: true }).fill("plugin-listener");
    await page
      .getByLabel("Password", { exact: true })
      .fill("plugin listener password");
    await page
      .getByRole("button", { name: "Create account", exact: true })
      .click();
    await expect(page.locator(".account-list")).toContainText(
      "plugin-listener",
    );
    await page.getByRole("button", { name: "Sign out", exact: true }).click();
    await signIn(page, "plugin-listener", "plugin listener password");
    await expect(
      page.getByRole("heading", {
        name: "Fixture collection updated",
        exact: true,
      }),
    ).toHaveCount(0);
    await expect(
      page.getByText("Services for this server", { exact: true }),
    ).toHaveCount(0);
    await page
      .getByRole("button", { name: "Connect service", exact: true })
      .click();
    await expect(
      page.getByRole("heading", {
        name: "Fixture collection updated",
        exact: true,
      }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Remove", exact: true }).click();
    await page
      .getByRole("button", { name: "Remove plugin", exact: true })
      .click();
    await expect(
      page.getByRole("heading", {
        name: "Fixture collection updated",
        exact: true,
      }),
    ).toHaveCount(0);
    await expect(
      page.getByRole("heading", { name: "Your plugins", exact: true }),
    ).toBeFocused();
    await page.getByRole("button", { name: "Sign out", exact: true }).click();
    await signIn(page, "admin", "docker browser test password");
    await expect(
      page.getByRole("heading", {
        name: "Fixture collection updated",
        exact: true,
      }),
    ).toBeVisible();
    await page.getByText("Services for this server", { exact: true }).click();
    await page
      .getByRole("button", { name: "Withdraw service", exact: true })
      .click();
    await page
      .getByRole("button", { name: "Withdraw for everyone", exact: true })
      .click();
    await expect(
      page.getByRole("heading", {
        name: "Fixture collection updated",
        exact: true,
      }),
    ).toHaveCount(0);
    await expect(
      page.getByRole("heading", { name: "Your plugins", exact: true }),
    ).toBeFocused();
    expect(errors).toEqual([]);
  } finally {
    await new Promise<void>((resolve, reject) =>
      service.close((error) => (error ? reject(error) : resolve())),
    );
  }
});
