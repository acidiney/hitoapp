import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { expect, test, type Page } from "@playwright/test";
import type { Schema } from "../src/shared/schema";

type Health = { state: string; reason: string | null; since: number | null };

async function stubPlugin(name: string, health: Health | null) {
  const manifest = {
    id: "org.hito.browser.health-fixture",
    name,
    protocol_version: 1,
    capabilities: ["search"],
  } satisfies Schema<"Manifest">;
  const paths: string[] = [];
  const server = createServer((request, response) => {
    const path = new URL(request.url ?? "/", "http://fixture.invalid").pathname;
    paths.push(path);
    const json = (value: unknown) =>
      response
        .writeHead(200, { "content-type": "application/json" })
        .end(JSON.stringify(value));
    if (path === "/fixture/v1/manifest") return json(manifest);
    if (path === "/fixture/v1/health" && health) return json(health);
    response.writeHead(404).end();
  });
  await new Promise<void>((resolve) =>
    server.listen(0, "127.0.0.1", () => resolve()),
  );
  const { port } = server.address() as AddressInfo;
  return {
    address: `http://127.0.0.1:${port}/fixture/`,
    paths,
    close: () => new Promise<void>((resolve) => server.close(() => resolve())),
  };
}

async function openPlugins(page: Page) {
  await page.goto("/settings?section=plugins");
  await expect(
    page.getByRole("heading", { name: "Your plugins", exact: true }),
  ).toBeVisible();
}

test("a plugin that cannot reach its provider shows a plain warning on its card and a healthy one does not", async ({
  page,
  baseURL,
}) => {
  const headers = { "x-hito-request": "1", origin: new URL(baseURL!).origin };
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const cases: {
    name: string;
    health: Health | null;
    expected: RegExp | null;
    shot: string;
  }[] = [
    {
      name: "Quiet service",
      health: null,
      expected: null,
      shot: "legacy",
    },
    {
      name: "Offline service",
      health: { state: "degraded", reason: "network", since: 1_700_000_000 },
      expected: /Hito cannot reach Offline service\./,
      shot: "network",
    },
    {
      name: "Blocked service",
      health: { state: "degraded", reason: "refused", since: 1_700_000_000 },
      expected: /Blocked service is refusing requests right now\./,
      shot: "refused",
    },
  ];
  for (const scheme of ["light", "dark"] as const) {
    await page.emulateMedia({ colorScheme: scheme });
    for (const entry of cases) {
      const stub = await stubPlugin(entry.name, entry.health);
      let installation: Schema<"PluginInstallation"> | null = null;
      let service: Schema<"PluginService"> | null = null;
      try {
        const offered = await page.request.post("/api/v1/plugins/services", {
          headers,
          data: {
            name: entry.name,
            base_url: stub.address,
          } satisfies Schema<"OfferPluginService">,
        });
        expect(offered.status()).toBe(201);
        service = await offered.json();
        const installed = await page.request.post("/api/v1/plugins", {
          headers,
          data: {
            service_id: service!.id,
            service_revision: service!.revision,
          } satisfies Schema<"InstallPlugin">,
        });
        expect(installed.status()).toBe(201);
        installation = await installed.json();

        await openPlugins(page);
        const card = page
          .locator("li.plugin-connection")
          .filter({ has: page.getByRole("heading", { name: entry.name }) });
        await expect(card).toBeVisible();
        if (entry.expected) {
          await expect(card.getByRole("status")).toContainText(entry.expected);
          await expect(card.getByRole("status")).toContainText(
            entry.health?.reason === "network"
              ? "Check the server's internet connection or DNS."
              : "Try again later.",
          );
        } else {
          await expect(card.getByRole("status")).toHaveCount(0);
        }
        await card.scrollIntoViewIfNeeded();
        await page.screenshot({
          path: `/workspace/test-results/plugin-health-${entry.shot}-${scheme}-desktop.png`,
        });
        await page.setViewportSize({ width: 390, height: 844 });
        await card.scrollIntoViewIfNeeded();
        expect(
          await page.evaluate(
            () => document.documentElement.scrollWidth > innerWidth,
          ),
        ).toBe(false);
        await page.screenshot({
          path: `/workspace/test-results/plugin-health-${entry.shot}-${scheme}-phone.png`,
        });
        await page.setViewportSize({ width: 1440, height: 1000 });
        expect(stub.paths.filter((path) => path.endsWith("/health"))).toEqual([
          "/fixture/v1/health",
        ]);
      } finally {
        if (installation)
          await page.request.delete(`/api/v1/plugins/${installation.id}`, {
            headers,
            data: { revision: installation.revision },
          });
        if (service)
          await page.request.delete(`/api/v1/plugins/services/${service.id}`, {
            headers,
            data: { revision: service.revision },
          });
        await stub.close();
      }
    }
  }
  expect(errors).toEqual([]);
});
