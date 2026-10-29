import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { isDeepStrictEqual } from "node:util";
import { expect, test, type Page } from "@playwright/test";
import type { components } from "../src/generated";
import {
  observePluginInstallations,
  reviewPluginInstallation,
} from "./fixtures/plugin-installation-review";

type Schema = components["schemas"];
test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});

async function pluginSettings(page: Page): Promise<Schema["PluginSettings"]> {
  const response = await page.request.get("/api/v1/plugins");
  try {
    expect(response.status()).toBe(200);
    return await response.json();
  } finally {
    await response.dispose();
  }
}

async function withdrawEveryService(page: Page) {
  const headers = {
    "x-hito-request": "1",
    origin: new URL(page.url()).origin,
  };
  for (const service of (await pluginSettings(page)).services) {
    const response = await page.request.delete(
      `/api/v1/plugins/services/${service.id}`,
      { headers, data: { revision: service.revision } },
    );
    expect(response.ok()).toBe(true);
  }
  await page.reload();
  await page
    .getByRole("navigation", { name: "Settings sections" })
    .getByRole("link", { name: "Plugins", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Your plugins", exact: true }),
  ).toBeVisible();
}

async function signIn(page: Page, username: string, password: string) {
  await page.goto("/settings");
  await page.getByLabel("Username", { exact: true }).fill(username);
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
  await openPlugins(page);
}

async function openPlugins(page: Page) {
  await page.goto("/settings");
  await page
    .getByRole("navigation", { name: "Settings sections" })
    .getByRole("link", { name: "Plugins", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Your plugins", exact: true }),
  ).toBeVisible();
}

test("an administrator offers only an address before connecting with the plugin name", async ({
  page,
}) => {
  const errors: string[] = [];
  const browserProviderRequests: string[] = [];
  const { evidence: installationRequests, stop: stopInstallationRequests } =
    observePluginInstallations(page);
  const requests: {
    authorization?: string;
    cookie?: string;
    protocol?: string | string[];
  }[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const manifest = {
    id: "org.hito.browser.automatic-name",
    name: "Automatic fixture provider",
    protocol_version: 1,
    capabilities: ["search", "sources"],
    content_policy: { source_persistence: "playback_only" },
  };
  const service = createServer((request, response) => {
    requests.push({
      authorization: request.headers.authorization,
      cookie: request.headers.cookie,
      protocol: request.headers["x-hito-plugin-protocol"],
    });
    if (request.url !== "/fixture/v1/manifest") {
      response.writeHead(404).end();
      return;
    }
    response
      .writeHead(200, { "content-type": "application/json" })
      .end(JSON.stringify(manifest));
  });
  await new Promise<void>((resolve) => service.listen(0, "127.0.0.1", resolve));
  const address = `http://127.0.0.1:${(service.address() as AddressInfo).port}/fixture`;
  page.on("request", (request) => {
    if (request.url().startsWith(address)) {
      browserProviderRequests.push(request.url());
    }
  });
  try {
    await openPlugins(page);
    await withdrawEveryService(page);
    await page.getByText("Services for this server", { exact: true }).click();
    const serviceAddress = page.getByLabel("Service address", { exact: true });
    await expect(serviceAddress).toHaveJSProperty("required", true);
    await expect(page.getByLabel("Service name", { exact: true })).toHaveCount(
      0,
    );
    await expect(
      page.getByLabel("Custom label (optional)", { exact: true }),
    ).toBeHidden();
    await serviceAddress.fill(address);
    const offered = page.waitForRequest(
      (request) =>
        request.method() === "POST" &&
        new URL(request.url()).pathname === "/api/v1/plugins/services",
    );
    const offerResponse = page.waitForResponse(
      (response) =>
        response.request().method() === "POST" &&
        new URL(response.url()).pathname === "/api/v1/plugins/services",
    );
    await page
      .getByRole("button", { name: "Offer service", exact: true })
      .click();
    expect((await offered).postDataJSON()).toEqual({ base_url: address });
    const acceptedOffer = await offerResponse;
    expect(acceptedOffer.status()).toBe(201);
    const offeredService: Schema["PluginService"] = await acceptedOffer.json();
    const selector = page.getByRole("combobox", {
      name: "Service",
      exact: true,
    });
    await expect(selector).toContainText(manifest.name);
    await expect(page.getByText(`${address}/`, { exact: true })).toHaveCount(2);
    await expect(
      page.getByText("No plugins connected to this account.", { exact: true }),
    ).toBeVisible();
    expect(requests).toEqual([
      { authorization: undefined, cookie: undefined, protocol: "1" },
    ]);
    const beforeReview = await pluginSettings(page);
    const reviewed = await reviewPluginInstallation(
      page,
      offeredService,
      manifest.name,
    );
    expect(installationRequests.total).toBe(0);
    expect(isDeepStrictEqual(await pluginSettings(page), beforeReview)).toBe(
      true,
    );
    await expect(
      page.getByText("No plugins connected to this account.", { exact: true }),
    ).toBeVisible();
    expect(requests).toEqual([
      { authorization: undefined, cookie: undefined, protocol: "1" },
      { authorization: undefined, cookie: undefined, protocol: "1" },
    ]);
    await page
      .getByRole("button", { name: "Accept and connect", exact: true })
      .click();
    await expect(
      page.getByRole("button", { name: "Check connection", exact: true }),
    ).toBeVisible();
    await expect(
      page.getByText("No plugins connected to this account.", { exact: true }),
    ).toHaveCount(0);
    expect(installationRequests.total).toBe(1);
    expect(
      isDeepStrictEqual(installationRequests.samples[0], {
        ...reviewed.sent,
        accept_manifest_digest: reviewed.body.digest,
      }),
    ).toBe(true);
    expect(requests).toEqual([
      { authorization: undefined, cookie: undefined, protocol: "1" },
      { authorization: undefined, cookie: undefined, protocol: "1" },
      { authorization: undefined, cookie: undefined, protocol: "1" },
    ]);
    expect(browserProviderRequests).toEqual([]);
    await page
      .getByRole("button", { name: "Withdraw service", exact: true })
      .click();
    await page
      .getByRole("button", { name: "Withdraw for everyone", exact: true })
      .click();
    await expect(selector).toHaveCount(0);
    expect(errors).toEqual([]);
    expect(installationRequests.overflow).toBe(false);
  } finally {
    stopInstallationRequests();
    await new Promise<void>((resolve, reject) =>
      service.close((error) => (error ? reject(error) : resolve())),
    );
  }
});

test.describe("account switching", () => {
  test.use({ storageState: { cookies: [], origins: [] } });
  test("personal plugin connections review changes and remain private across accounts", async ({
    page,
  }) => {
    const errors: string[] = [];
    const { evidence: installationRequests, stop: stopInstallationRequests } =
      observePluginInstallations(page);
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
    await new Promise<void>((resolve) =>
      service.listen(0, "127.0.0.1", resolve),
    );
    const address = `http://127.0.0.1:${(service.address() as AddressInfo).port}/fixture/`;
    try {
      await signIn(page, "admin", "docker browser test password");
      await withdrawEveryService(page);
      await page.getByText("Services for this server", { exact: true }).click();
      await page.getByText("Advanced settings", { exact: true }).click();
      await page
        .getByLabel("Custom label (optional)", { exact: true })
        .fill("Browser fixture service");
      await page.getByLabel("Service address", { exact: true }).fill(address);
      const offerResponse = page.waitForResponse(
        (response) =>
          response.request().method() === "POST" &&
          new URL(response.url()).pathname === "/api/v1/plugins/services",
      );
      await page
        .getByRole("button", { name: "Offer service", exact: true })
        .click();
      const acceptedOffer = await offerResponse;
      expect(acceptedOffer.status()).toBe(201);
      const offeredService: Schema["PluginService"] =
        await acceptedOffer.json();
      await expect(
        page.getByRole("combobox", { name: "Service", exact: true }),
      ).toHaveValue(/.+/);
      expect(requests).toEqual([]);
      await page.setViewportSize({ width: 390, height: 844 });
      const reviewButton = page.getByRole("button", {
        name: "Review connection",
        exact: true,
      });
      await reviewButton.scrollIntoViewIfNeeded();
      const connectBounds = await reviewButton.boundingBox();
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
      const serviceAddress = page.getByLabel("Service address", {
        exact: true,
      });
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
      const beforeReview = await pluginSettings(page);
      await reviewPluginInstallation(page, offeredService, manifest.name);
      expect(installationRequests.total).toBe(0);
      expect(isDeepStrictEqual(await pluginSettings(page), beforeReview)).toBe(
        true,
      );
      expect(
        (
          await page.evaluate(() =>
            JSON.stringify({
              local: Object.entries(localStorage),
              session: Object.entries(sessionStorage),
            }),
          )
        ).includes(expectedToken!),
      ).toBe(false);
      await page.getByRole("button", { name: "Back", exact: true }).click();
      await expect(
        page.getByRole("heading", { name: "Review connection", exact: true }),
      ).toHaveCount(0);
      expect(installationRequests.total).toBe(0);
      expect(isDeepStrictEqual(await pluginSettings(page), beforeReview)).toBe(
        true,
      );
      expect(
        requests.length === 1 && requests[0] === `Bearer ${expectedToken}`,
      ).toBe(true);
      expect(
        (
          await page.evaluate(() =>
            JSON.stringify({
              local: Object.entries(localStorage),
              session: Object.entries(sessionStorage),
            }),
          )
        ).includes(expectedToken!),
      ).toBe(false);
      const reviewed = await reviewPluginInstallation(
        page,
        offeredService,
        manifest.name,
      );
      expect(installationRequests.total).toBe(0);
      expect(
        requests.length === 2 &&
          requests.every((token) => token === `Bearer ${expectedToken}`),
      ).toBe(true);
      await page
        .getByRole("button", { name: "Accept and connect", exact: true })
        .click();
      await expect(
        page.getByRole("heading", { name: "Fixture collection", exact: true }),
      ).toBeVisible();
      expect(installationRequests.total).toBe(1);
      expect(
        isDeepStrictEqual(installationRequests.samples[0], {
          ...reviewed.sent,
          accept_manifest_digest: reviewed.body.digest,
        }),
      ).toBe(true);
      expect(
        requests.length === 3 &&
          requests.every((token) => token === `Bearer ${expectedToken}`),
      ).toBe(true);
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
        page.getByRole("heading", {
          name: "Review plugin changes",
          exact: true,
        }),
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
        page.getByRole("heading", {
          name: "Review plugin changes",
          exact: true,
        }),
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
      await page
        .getByRole("button", { name: "Save token", exact: true })
        .click();
      await expect(page.getByRole("alert")).toContainText("rejected");
      await expect(
        page.getByLabel("New bearer token", { exact: true }),
      ).toHaveValue("wrong-token");
      expectedToken = "replacement-browser-token";
      await page
        .getByLabel("New bearer token", { exact: true })
        .fill(expectedToken);
      await page
        .getByRole("button", { name: "Save token", exact: true })
        .click();
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
      await page
        .getByRole("navigation", { name: "Settings sections" })
        .getByRole("link", { name: "Accounts", exact: true })
        .click();
      await page
        .getByRole("button", { name: "Create account", exact: true })
        .click();
      const dialog = page.getByRole("dialog", { name: "Create account" });
      await dialog
        .getByLabel("Username", { exact: true })
        .fill("plugin-listener");
      await dialog
        .getByLabel("Password", { exact: true })
        .fill("plugin listener password");
      await dialog
        .getByRole("button", { name: "Create account", exact: true })
        .click();
      await expect(
        page.getByRole("region", { name: "Accounts and access" }),
      ).toContainText("plugin-listener");
      await page.getByRole("button", { name: "Sign out", exact: true }).click();
      await expect(
        page.getByRole("heading", { name: "Welcome back" }),
      ).toBeVisible();
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
      const listenerReview = await reviewPluginInstallation(
        page,
        offeredService,
        manifest.name,
      );
      await page
        .getByRole("button", { name: "Accept and connect", exact: true })
        .click();
      await expect(
        page.getByRole("heading", {
          name: "Fixture collection updated",
          exact: true,
        }),
      ).toBeVisible();
      expect(installationRequests.total).toBe(2);
      expect(
        isDeepStrictEqual(installationRequests.samples[1], {
          ...listenerReview.sent,
          accept_manifest_digest: listenerReview.body.digest,
        }),
      ).toBe(true);
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
      await expect(
        page.getByRole("heading", { name: "Welcome back" }),
      ).toBeVisible();
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
      expect(installationRequests.overflow).toBe(false);
    } finally {
      stopInstallationRequests();
      await new Promise<void>((resolve, reject) =>
        service.close((error) => (error ? reject(error) : resolve())),
      );
    }
  });
});
