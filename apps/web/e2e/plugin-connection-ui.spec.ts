import http from "node:http";
import type { AddressInfo } from "node:net";
import { expect, type BrowserContext, type Page, test } from "@playwright/test";
import { adminCredentials, enterAdmin } from "./fixtures/admin-session";

test.skip(
  process.env.HITO_PLUGIN_AUTHORIZATION_E2E !== "1",
  "Requires the loopback-origin provider authorization gate",
);

const ACCESS_TOKEN = "consent-access-token-value";
const REFRESH_TOKEN = "consent-refresh-token-value";
const headers = { "x-hito-request": "1" };
const other = {
  username: "consent-other",
  password: "other account test password",
};

type Fixture = Awaited<ReturnType<typeof startProvider>>;

async function startProvider(name: string, baseURL: string) {
  const tokenRequests: string[] = [];
  const authorizeQueries: URLSearchParams[] = [];
  const server = http.createServer((incoming, outgoing) => {
    const chunks: Buffer[] = [];
    incoming.on("data", (chunk) => chunks.push(chunk));
    incoming.on("end", () => {
      const url = new URL(incoming.url ?? "/", "http://provider.test");
      const port = (server.address() as AddressInfo).port;
      if (url.pathname === "/token") {
        tokenRequests.push(Buffer.concat(chunks).toString());
        outgoing.setHeader("content-type", "application/json");
        outgoing.end(
          JSON.stringify({
            access_token: ACCESS_TOKEN,
            refresh_token: REFRESH_TOKEN,
            token_type: "Bearer",
            expires_in: 3600,
          }),
        );
        return;
      }
      outgoing.setHeader(
        "content-type",
        url.pathname === "/authorize" ? "text/html" : "application/json",
      );
      if (url.pathname === "/authorize") {
        authorizeQueries.push(url.searchParams);
        const redirect = url.searchParams.get("redirect_uri") ?? "";
        const state = url.searchParams.get("state") ?? "";
        outgoing.end(
          `<!doctype html><title>Fake provider</title><h1>Fake provider consent</h1><p>${name} wants to read your playlists.</p><a id="approve" href="${redirect}?code=owned-consent-code&state=${state}">Approve</a> <a id="deny" href="${redirect}?error=access_denied&state=${state}">Deny</a>`,
        );
        return;
      }
      outgoing.end(
        JSON.stringify({
          id: `org.hito.consent-${name.replaceAll(" ", "-").toLowerCase()}`,
          name,
          protocol_version: 1,
          capabilities: ["playlists"],
          authorization: {
            kind: "oauth2_pkce",
            client_id: "consent-client",
            authorization_endpoint: `http://127.0.0.1:${port}/authorize`,
            token_endpoint: `http://127.0.0.1:${port}/token`,
            scopes: ["playlist-read-private", "user-library-read"],
          },
        }),
      );
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  return {
    name,
    baseURL,
    address: `http://127.0.0.1:${(server.address() as AddressInfo).port}`,
    tokenRequests,
    authorizeQueries,
    close: () => new Promise<void>((resolve) => server.close(() => resolve())),
  };
}

async function post(page: Page, path: string, data: unknown) {
  return page.request.post(path, {
    headers: { ...headers, origin: new URL(page.url()).origin },
    data,
  });
}

async function install(page: Page, service: { id: string; revision: number }) {
  const installed = await post(page, "/api/v1/plugins", {
    service_id: service.id,
    service_revision: service.revision,
  });
  expect(installed.status()).toBe(201);
}

function row(page: Page, name: string) {
  return page
    .locator(".plugin-connection")
    .filter({ has: page.getByRole("heading", { name, exact: true }) });
}
async function openPlugins(page: Page, name: string) {
  await page.goto("/settings?section=plugins");
  await expect(
    page.getByRole("heading", { name: "Your plugins", exact: true }),
  ).toBeVisible();
  await expect(row(page, name)).toBeVisible();
}
const signInStatus = (page: Page, name: string) =>
  row(page, name).getByRole("status", { name: "Sign-in status" });

function connectButton(page: Page, name: string) {
  return row(page, name).getByRole("button", {
    name: "Connect",
    exact: true,
  });
}

async function consent(
  page: Page,
  context: BrowserContext,
  name: string,
  fixture: Fixture,
) {
  const opened = context.waitForEvent("page");
  await connectButton(page, name).click();
  const provider = await opened;
  await provider.waitForURL(/\/authorize\?/);
  await expect(
    provider.getByRole("heading", { name: "Fake provider consent" }),
  ).toBeVisible();
  expect(await provider.evaluate(() => window.opener)).toBeNull();
  await expect(signInStatus(page, name)).toHaveText(`Waiting for ${name}…`);
  expect(page.url()).not.toMatch(/state|attempt|code/);
  const query = fixture.authorizeQueries.at(-1)!;
  expect(query.get("response_type")).toBe("code");
  expect(query.get("code_challenge_method")).toBe("S256");
  expect(query.get("redirect_uri")).toBe(
    `${fixture.baseURL}/api/v1/plugin-authorization/callback`,
  );
  expect(query.get("scope")).toBe("playlist-read-private user-library-read");
  await provider.getByRole("link", { name: "Approve" }).click();
  await expect(
    provider.getByRole("heading", { name: "Provider authorized" }),
  ).toBeVisible();
  await provider.close();
  await page.bringToFront();
  await expect(signInStatus(page, name)).toHaveText("Connected", {
    timeout: 20_000,
  });
  await expect(
    row(page, name).getByText(
      "Access: playlist read private, user library read",
    ),
  ).toBeVisible();
}

async function switchAccount(
  page: Page,
  credentials: { username: string; password: string },
) {
  await page.getByRole("button", { name: "Sign out", exact: true }).click();
  await page.getByLabel("Username", { exact: true }).fill(credentials.username);
  await page.getByLabel("Password", { exact: true }).fill(credentials.password);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
}

test("the consent flow connects through the provider page, survives a refusal and a reauthorization notice, disconnects and stays private to each account", async ({
  page,
  context,
  baseURL,
  browserName,
}) => {
  const name = `Consent fixture ${browserName}`;
  const fixture = await startProvider(name, baseURL!);
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));

  await enterAdmin(page);
  const created = await post(page, "/api/v1/users", other);
  expect([201, 409]).toContain(created.status());
  const offered = await post(page, "/api/v1/plugins/services", {
    name,
    base_url: fixture.address,
  });
  expect(offered.status()).toBe(201);
  const service = await offered.json();
  await install(page, service);

  await openPlugins(page, name);
  await expect(signInStatus(page, name)).toHaveText("Not connected");
  await expect(
    row(page, name).getByRole("button", { name: "Disconnect" }),
  ).toHaveCount(0);

  const refused = context.waitForEvent("page");
  await connectButton(page, name).click();
  const refusal = await refused;
  await refusal.waitForURL(/\/authorize\?/);
  await refusal.getByRole("link", { name: "Deny" }).click();
  await expect(
    refusal.getByRole("heading", { name: "Connection declined" }),
  ).toBeVisible();
  await refusal.close();
  await page.bringToFront();
  await expect(row(page, name).getByRole("alert")).toContainText(
    "did not finish",
    { timeout: 20_000 },
  );
  await expect(signInStatus(page, name)).toHaveText("Not connected");
  expect(fixture.tokenRequests).toHaveLength(0);

  await consent(page, context, name, fixture);
  expect(fixture.tokenRequests).toHaveLength(1);
  expect(await page.content()).not.toContain(ACCESS_TOKEN);
  expect(await page.content()).not.toContain(REFRESH_TOKEN);
  await page.reload();
  await expect(signInStatus(page, name)).toHaveText("Connected");

  await page.route("**/api/v1/plugins/*/connection", (route) =>
    route.request().method() === "GET"
      ? route.fulfill({ json: { status: "needs_reauthorization", scopes: [] } })
      : route.continue(),
  );
  await page.reload();
  await expect(signInStatus(page, name)).toHaveText("Sign in again needed");
  await expect(
    row(page, name).getByRole("button", { name: "Reconnect" }),
  ).toBeVisible();
  await page.unroute("**/api/v1/plugins/*/connection");

  await switchAccount(page, other);
  await install(page, service);
  await openPlugins(page, name);
  await expect(signInStatus(page, name)).toHaveText("Not connected");
  await expect(
    row(page, name).getByRole("button", { name: "Disconnect" }),
  ).toHaveCount(0);

  await consent(page, context, name, fixture);
  expect(fixture.tokenRequests).toHaveLength(2);
  await row(page, name).getByRole("button", { name: "Disconnect" }).click();
  const dialog = page.getByRole("dialog", { name: `Disconnect ${name}?` });
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(signInStatus(page, name)).toHaveText("Connected");
  await row(page, name).getByRole("button", { name: "Disconnect" }).click();
  await dialog.getByRole("button", { name: "Disconnect" }).click();
  await expect(dialog).toBeHidden();
  await expect(signInStatus(page, name)).toHaveText("Not connected");
  await page.reload();
  await expect(signInStatus(page, name)).toHaveText("Not connected");

  await switchAccount(page, adminCredentials);
  await openPlugins(page, name);
  await expect(signInStatus(page, name)).toHaveText("Connected");
  expect(errors).toEqual([]);
  await fixture.close();
});
