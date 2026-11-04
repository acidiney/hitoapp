import { mkdir } from "node:fs/promises";
import http from "node:http";
import type { AddressInfo } from "node:net";
import { expect, request, test } from "@playwright/test";
import { adminCredentials } from "./fixtures/admin-session";

test.skip(
  process.env.HITO_PLUGIN_AUTHORIZATION_E2E !== "1",
  "Requires the loopback-origin provider authorization gate",
);

const evidence =
  process.env.HITO_PLUGIN_AUTHORIZATION_EVIDENCE ?? "test-results/oauth";
const CODE = "owned-browser-code";

type Listener = { port: number; close: () => Promise<void> };

async function listen(
  handler: http.RequestListener,
): Promise<Listener & { requests: string[] }> {
  const requests: string[] = [];
  const server = http.createServer((incoming, outgoing) => {
    const chunks: Buffer[] = [];
    incoming.on("data", (chunk) => chunks.push(chunk));
    incoming.on("end", () => {
      requests.push(Buffer.concat(chunks).toString());
      handler(incoming, outgoing);
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  return {
    port: (server.address() as AddressInfo).port,
    requests,
    close: () => new Promise((resolve) => server.close(() => resolve())),
  };
}

test("a provider redirect from another site reaches the callback with the session and renders each outcome", async ({
  browser,
  browserName,
  baseURL,
}) => {
  await mkdir(evidence, { recursive: true });
  const token = await listen((_, outgoing) => {
    outgoing.setHeader("content-type", "application/json");
    outgoing.end(
      JSON.stringify({
        access_token: "owned-browser-access-token",
        token_type: "Bearer",
        expires_in: 3600,
      }),
    );
  });
  const manifest = await listen((_, outgoing) => {
    outgoing.setHeader("content-type", "application/json");
    outgoing.end(
      JSON.stringify({
        id: `org.hito.callback-browser-${browserName}`,
        name: "Callback browser fixture",
        protocol_version: 1,
        capabilities: ["playlists"],
        authorization: {
          kind: "oauth2_pkce",
          client_id: "owned-browser-client",
          authorization_endpoint: "https://accounts.spotify.com/authorize",
          token_endpoint: `http://127.0.0.1:${token.port}/token`,
          scopes: ["playlist-read-private"],
        },
      }),
    );
  });
  const otherSite = await listen((incoming, outgoing) => {
    const target = new URL(incoming.url ?? "/", "http://other.test").searchParams.get("to");
    outgoing.setHeader("content-type", "text/html");
    outgoing.end(
      `<!doctype html><title>Provider</title><p>Provider consent finished.</p><a id="go" href="${target}">Return to Hito</a>`,
    );
  });
  const api = await request.newContext({ baseURL: baseURL! });
  const headers = { "x-hito-request": "1" };
  let session = await api.post("/api/v1/setup", { headers, data: adminCredentials });
  if (!session.ok()) {
    session = await api.post("/api/v1/login", { headers, data: adminCredentials });
  }
  expect(session.ok()).toBe(true);
  const bearer = (await session.json()).token as string;
  const auth = { ...headers, authorization: `Bearer ${bearer}` };
  const service = await (
    await api.post("/api/v1/plugins/services", {
      headers: auth,
      data: {
        name: `Callback fixture ${browserName}`,
        base_url: `http://127.0.0.1:${manifest.port}`,
      },
    })
  ).json();
  const installationResponse = await api.post("/api/v1/plugins", {
    headers: auth,
    data: { service_id: service.id, service_revision: service.revision },
  });
  expect(installationResponse.status()).toBe(201);
  const installation = await installationResponse.json();
  const begin = async () => {
    const response = await api.post(
      `/api/v1/plugins/${installation.id}/authorization`,
      { headers: auth, data: { revision: installation.revision } },
    );
    expect(response.status()).toBe(201);
    const attempt = await response.json();
    return {
      id: attempt.id as string,
      expiresAt: attempt.expires_at as number,
      state: new URL(attempt.authorization_url).searchParams.get("state")!,
    };
  };
  const status = async (id: string) =>
    (
      await api.get(`/api/v1/plugins/${installation.id}/authorization/${id}`, {
        headers: auth,
      })
    ).status();

  const expiring = await begin();
  const connected = await begin();
  const declined = await begin();

  const context = await browser.newContext();
  await context.addCookies([
    {
      name: "hito_session",
      value: bearer,
      domain: "127.0.0.1",
      path: "/",
      httpOnly: true,
      sameSite: "Strict",
    },
  ]);
  const page = await context.newPage();
  const callback = (query: string) =>
    `${baseURL}/api/v1/plugin-authorization/callback?${query}`;
  const arrive = async (query: string) => {
    const responses: number[] = [];
    page.on("response", (response) => {
      if (response.url().includes("/plugin-authorization/callback")) {
        responses.push(response.status());
      }
    });
    await page.goto(
      `http://localhost:${otherSite.port}/?to=${encodeURIComponent(callback(query))}`,
    );
    await page.locator("#go").click();
    return responses;
  };

  const responses = await arrive(`code=${CODE}&state=${connected.state}`);
  await expect(page.getByRole("heading", { name: "Provider authorized" })).toBeVisible();
  expect(responses).toEqual([200, 200]);
  expect(page.url()).toContain("/api/v1/plugin-authorization/callback");
  expect(token.requests).toHaveLength(1);
  expect(token.requests[0]).toContain(`code=${CODE}`);
  expect(token.requests[0]).toContain("code_verifier=");
  await page.screenshot({ path: `${evidence}/${browserName}-connected.png` });
  const completed = await api.get(
    `/api/v1/plugins/${installation.id}/authorization/${connected.id}`,
    { headers: auth },
  );
  expect((await completed.json()).status).toBe("completed");

  await page.goto(callback(`code=${CODE}&state=${connected.state}`));
  await expect(
    page.getByRole("heading", { name: "This connection attempt was already used" }),
  ).toBeVisible();
  expect(token.requests).toHaveLength(1);
  await page.screenshot({ path: `${evidence}/${browserName}-already-used.png` });

  await arrive(`error=access_denied&state=${declined.state}`);
  await expect(page.getByRole("heading", { name: "Connection declined" })).toBeVisible();
  await page.screenshot({ path: `${evidence}/${browserName}-declined.png` });
  expect(await status(declined.id)).toBe(200);

  const wait = expiring.expiresAt * 1000 - Date.now() + 3000;
  if (wait > 0) await page.waitForTimeout(wait);
  await arrive(`code=${CODE}&state=${expiring.state}`);
  await expect(
    page.getByRole("heading", { name: "This connection attempt expired" }),
  ).toBeVisible();
  await page.screenshot({ path: `${evidence}/${browserName}-expired.png` });
  expect(token.requests).toHaveLength(1);

  const signedOut = await browser.newContext();
  const bare = await signedOut.newPage();
  await bare.goto(
    `http://localhost:${otherSite.port}/?to=${encodeURIComponent(callback(`code=${CODE}&state=${connected.state}`))}`,
  );
  await bare.locator("#go").click();
  await expect(bare.getByRole("heading", { name: "Sign in to Hito" })).toBeVisible();
  await bare.screenshot({ path: `${evidence}/${browserName}-signed-out.png` });

  await signedOut.close();
  await context.close();
  await api.dispose();
  await Promise.all([token.close(), manifest.close(), otherSite.close()]);
});
