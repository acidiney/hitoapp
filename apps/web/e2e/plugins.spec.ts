import { openAllManage } from "./fixtures/plugin-ui";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { isDeepStrictEqual } from "node:util";
import { expect, test, type Page } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import {
  observePluginInstallations,
  reviewPluginInstallation,
} from "./fixtures/plugin-installation-review";

test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});

async function pluginSettings(page: Page): Promise<Schema<"PluginSettings">> {
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
  await expect(
    page.getByRole("heading", { name: "Plugins", exact: true }),
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
  await page.goto("/settings?section=plugins");
  await expect(
    page.getByRole("heading", { name: "Plugins", exact: true }),
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
    if (request.url === "/fixture/v1/health") {
      response.writeHead(404).end();
      return;
    }
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
    await page.getByRole("button", { name: /^This server/ }).click();
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
    const offeredService: Schema<"PluginService"> = await acceptedOffer.json();
    await expect(page.getByText(`${address}/`, { exact: true })).toHaveCount(1);
    await page
      .getByRole("button", { name: "Your plugins", exact: true })
      .click();
    await page
      .getByRole("button", { name: `Connect ${manifest.name}`, exact: true })
      .click();
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
      page.getByText(/Your library plays local files on its own/),
    ).toBeVisible();
    expect(requests).toEqual([
      { authorization: undefined, cookie: undefined, protocol: "1" },
      { authorization: undefined, cookie: undefined, protocol: "1" },
    ]);
    await page
      .getByRole("button", { name: "Accept and connect", exact: true })
      .click();
    await openAllManage(page);
    await expect(
      page.getByRole("button", { name: "Check connection", exact: true }),
    ).toBeVisible();
    await expect(
      page.getByText(/Your library plays local files on its own/),
    ).toHaveCount(0);
    expect(installationRequests.total).toBe(1);
    expect(
      isDeepStrictEqual(installationRequests.samples[0], {
        ...reviewed.sent,
        accept_manifest_digest: reviewed.body.digest,
      }),
    ).toBe(true);
    await expect
      .poll(() => requests)
      .toEqual(
        Array.from({ length: 4 }, () => ({
          authorization: undefined,
          cookie: undefined,
          protocol: "1",
        })),
      );
    expect(browserProviderRequests).toEqual([]);
    await page.getByRole("button", { name: /^This server/ }).click();
    await page.getByRole("button", { name: /^Withdraw / }).click();
    await page
      .getByRole("button", { name: "Withdraw for everyone", exact: true })
      .click();
    await expect(page.getByRole("button", { name: /^Withdraw / })).toHaveCount(
      0,
    );
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
      if (request.url === "/fixture/v1/health") {
        response.writeHead(404).end();
        return;
      }
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
      await page.getByRole("button", { name: /^This server/ }).click();
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
      const offeredService: Schema<"PluginService"> =
        await acceptedOffer.json();
      await page
        .getByRole("button", { name: "Your plugins", exact: true })
        .click();
      await page
        .getByRole("button", {
          name: `Connect ${offeredService.name}`,
          exact: true,
        })
        .click();
      await expect(page.getByRole("dialog")).toBeVisible();
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
      const tokenField = await page
        .getByLabel("Bearer token (optional)", { exact: true })
        .boundingBox();
      expect(tokenField!.height).toBeGreaterThanOrEqual(44);
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth > innerWidth,
        ),
      ).toBe(false);
      await page.screenshot({
        path: "/workspace/test-results/plugins-connect-mobile.png",
      });
      await page.getByRole("button", { name: "Cancel", exact: true }).click();
      await page.getByRole("button", { name: /^This server/ }).click();
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
        .getByRole("button", { name: "Your plugins", exact: true })
        .click();
      await page
        .getByRole("button", {
          name: `Connect ${offeredService.name}`,
          exact: true,
        })
        .click();
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
      await expect
        .poll(
          () =>
            requests.length === 4 &&
            requests.every((token) => token === `Bearer ${expectedToken}`),
        )
        .toBe(true);
      expect(requests[0]).toBe(`Bearer ${expectedToken}`);
      manifest = {
        ...manifest,
        name: "Fixture collection updated",
        capabilities: ["metadata", "sources"],
      };
      await openAllManage(page);
      await page
        .getByRole("button", { name: "Check connection", exact: true })
        .click();
      await expect(page.getByRole("dialog")).toBeVisible();
      await page.screenshot({
        path: "/workspace/test-results/plugins-review-desktop.png",
      });
      await page.setViewportSize({ width: 390, height: 844 });
      await page
        .getByRole("button", { name: /^Accept / })
        .scrollIntoViewIfNeeded();
      await page.screenshot({
        path: "/workspace/test-results/plugins-review-mobile.png",
      });
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth > innerWidth,
        ),
      ).toBe(false);
      await page.getByRole("button", { name: /^Accept / }).click();
      await expect(page.getByRole("dialog")).toHaveCount(0);
      await expect(
        page.getByRole("heading", {
          name: "Fixture collection updated",
          exact: true,
        }),
      ).toBeVisible();
      await openAllManage(page);
      await expect(
        page.getByRole("button", { name: "Check connection", exact: true }),
      ).toBeFocused();
      await page.setViewportSize({ width: 1440, height: 1000 });
      await openAllManage(page);
      await page
        .getByLabel("New bearer token", { exact: true })
        .fill("wrong-token");
      await page
        .getByRole("button", { name: "Save token", exact: true })
        .click();
      await expect(page.getByRole("alert")).toContainText(
        "The plugin service refused the request",
      );
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
      await openAllManage(page);
      await expect(
        page.getByText("No token saved", { exact: true }),
      ).toBeVisible();
      expect(requests.length).toBe(beforeClear);
      await openAllManage(page);
      await expect(
        page.getByRole("button", { name: "Check connection", exact: true }),
      ).toBeFocused();
      expectedToken = null;
      await openAllManage(page);
      await page
        .getByRole("button", { name: "Check connection", exact: true })
        .click();
      await expect.poll(() => requests.length).toBe(beforeClear + 1);
      await openAllManage(page);
      await expect(
        page.getByRole("button", { name: "Check connection", exact: true }),
      ).toBeEnabled();
      await page.screenshot({
        path: "/workspace/test-results/plugins-connected-desktop.png",
      });
      await page.goto("/settings?section=accounts");
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
        page.getByRole("heading", { name: "Sign in", exact: true }),
      ).toBeVisible();
      await signIn(page, "plugin-listener", "plugin listener password");
      await expect(
        page.getByRole("heading", {
          name: "Fixture collection updated",
          exact: true,
        }),
      ).toHaveCount(0);
      await expect(
        page.getByRole("button", { name: /^This server/ }),
      ).toHaveCount(0);
      await page
        .getByRole("button", {
          name: `Connect ${offeredService.name}`,
          exact: true,
        })
        .click();
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
      await openAllManage(page);
      await page.getByRole("button", { name: "Remove", exact: true }).click();
      await openAllManage(page);
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
        page.getByRole("heading", { name: "Plugins", exact: true }),
      ).toBeFocused();
      await page.getByRole("button", { name: "Sign out", exact: true }).click();
      await expect(
        page.getByRole("heading", { name: "Sign in", exact: true }),
      ).toBeVisible();
      await signIn(page, "admin", "docker browser test password");
      await expect(
        page.getByRole("heading", {
          name: "Fixture collection updated",
          exact: true,
        }),
      ).toBeVisible();
      await page.getByRole("button", { name: /^This server/ }).click();
      await page.getByRole("button", { name: /^Withdraw / }).click();
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
        page.getByRole("heading", { name: "Plugins", exact: true }),
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
