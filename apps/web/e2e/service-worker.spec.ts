import { networkInterfaces } from "node:os";
import { expect, test, type Page } from "@playwright/test";
import { signOutFromShell } from "./fixtures/account";
import { createMembers, memberPassword } from "./fixtures/member-accounts";
import { workerProxy } from "./fixtures/worker-proxy";

test.use({
  serviceWorkers: "allow",
  storageState: { cookies: [], origins: [] },
});

const hashedAsset =
  /^\/assets\/[^/]+-[A-Za-z0-9_-]{8}\.(?:js|css|woff2|svg|png)$/;
const publicStatic = ["/icon-192.png", "/favicon.svg"];
const offlineKey = "/__hito-offline";

type Entry = { cache: string; path: string; type: string; mentions: string[] };

async function controlled(page: Page) {
  await page.evaluate(() => navigator.serviceWorker.ready);
  await expect
    .poll(
      () => page.evaluate(() => navigator.serviceWorker.controller !== null),
      { timeout: 20_000 },
    )
    .toBe(true);
}

async function cached(page: Page, secrets: string[]): Promise<Entry[]> {
  return page.evaluate(async (needles) => {
    const entries: Entry[] = [];
    for (const cache of await caches.keys()) {
      const store = await caches.open(cache);
      for (const request of await store.keys()) {
        const response = (await store.match(request))!;
        const type = response.headers.get("content-type") ?? "";
        const text = /javascript|css|html|json|svg|text/.test(type)
          ? await response.clone().text()
          : "";
        const url = new URL(request.url);
        entries.push({
          cache,
          path: url.pathname + url.search,
          type,
          mentions: needles.filter((needle) => text.includes(needle)),
        });
      }
    }
    return entries;
  }, secrets);
}

function expectOnlyPublicStatic(entries: Entry[]) {
  expect(entries.length).toBeGreaterThan(0);
  for (const entry of entries) {
    expect(
      hashedAsset.test(entry.path) ||
        publicStatic.includes(entry.path) ||
        entry.path === offlineKey,
      `unexpected cache entry ${entry.path}`,
    ).toBe(true);
    expect(entry.path).not.toContain("/api/");
    expect(entry.path).not.toContain("?");
    expect(entry.type).not.toMatch(/json|audio|video|octet-stream/);
    if (entry.path !== offlineKey) expect(entry.type).not.toMatch(/html/);
    expect(entry.mentions).toEqual([]);
  }
  expect(entries.some((entry) => entry.path.endsWith(".woff2"))).toBe(true);
  expect(entries.map((entry) => entry.path)).not.toContain("/assets/app.js");
  expect(entries.map((entry) => entry.path)).not.toContain("/assets/app.css");
}

test("the worker installs and controls the page, caches only public static files and keeps no private data after sign out", async ({
  page,
  browser,
  baseURL,
}) => {
  const members = await createMembers(browser, baseURL!, "worker", 1);
  const [username] = members.names;
  const audio: string[] = [];
  const failures: string[] = [];
  page.on("response", (response) => {
    if ((response.headers()["content-type"] ?? "").startsWith("audio/"))
      audio.push(new URL(response.url()).pathname);
  });
  page.on("pageerror", (error) => failures.push(error.message));
  try {
    await page.goto("/");
    await page.getByLabel("Username", { exact: true }).fill(username);
    await page.getByLabel("Password", { exact: true }).fill(memberPassword);
    await page.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();

    expect(await page.evaluate(() => window.isSecureContext)).toBe(true);
    await controlled(page);
    const registration = await page.evaluate(async () => {
      const found = (await navigator.serviceWorker.getRegistration())!;
      return {
        scope: new URL(found.scope).pathname,
        script: new URL(found.active!.scriptURL).pathname,
        state: found.active!.state,
      };
    });
    expect(registration).toEqual({
      scope: "/",
      script: "/sw.js",
      state: "activated",
    });

    await page.reload();
    await controlled(page);
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    await page
      .getByRole("button", { name: "Play Fixture 440 Hz", exact: true })
      .click();
    await expect
      .poll(() =>
        page
          .locator("audio[data-hito-player]")
          .evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(0.2);
    expect(audio.length).toBeGreaterThan(0);
    await page.getByRole("link", { name: "Home", exact: true }).first().click();
    await expect(page).toHaveURL(/\/home$/);
    await page.goto("/settings");
    await expect(page.locator(".account")).toContainText(username);

    const secrets = [username];
    const signedIn = await cached(page, secrets);
    expectOnlyPublicStatic(signedIn);
    for (const path of audio)
      expect(signedIn.map((entry) => entry.path)).not.toContain(path);
    const offline = signedIn.find((entry) => entry.path === offlineKey);
    expect(offline?.type).toContain("text/html");

    await signOutFromShell(page);
    await expect(
      page.getByRole("heading", { name: "Sign in", exact: true }),
    ).toBeVisible();
    expectOnlyPublicStatic(await cached(page, secrets));

    await page.goto("/library");
    await expect(
      page.getByRole("heading", { name: "Sign in", exact: true }),
    ).toBeVisible();
    await expect(page.locator("body")).not.toContainText(username);
    expectOnlyPublicStatic(await cached(page, secrets));
    expect(failures).toEqual([]);
  } finally {
    await members.close();
  }
});

test("an updated worker is noticed through the real registration and offered with a reload toast", async ({
  page,
  baseURL,
}) => {
  const proxy = await workerProxy(baseURL!);
  try {
    await page.goto(`${proxy.origin}/`);
    await expect(
      page.getByRole("heading", { name: "Sign in", exact: true }),
    ).toBeVisible();
    expect(await page.evaluate(() => window.isSecureContext)).toBe(true);
    await controlled(page);
    const toast = page
      .getByRole("status")
      .filter({ hasText: "A new version of Hito is ready" });

    const unchanged = await page.evaluate(async () => {
      const found = (await navigator.serviceWorker.getRegistration())!;
      await found.update();
      return {
        waiting: found.waiting !== null,
        installing: found.installing !== null,
      };
    });
    expect(unchanged).toEqual({ waiting: false, installing: false });
    await expect(toast).toHaveCount(0);

    proxy.state.changed = true;
    await page.evaluate(async () => {
      await (await navigator.serviceWorker.getRegistration())!.update();
    });
    await expect(toast).toBeVisible();
    expect(proxy.state.workerRequests).toBeGreaterThanOrEqual(2);
    expect(
      await page.evaluate(
        async () =>
          (await navigator.serviceWorker.getRegistration())!.waiting !== null,
      ),
    ).toBe(true);

    const reloaded = page.waitForEvent("load");
    await toast.getByRole("button", { name: "Reload" }).click();
    await reloaded;
    await expect(toast).toHaveCount(0);
    await controlled(page);
    await expect
      .poll(() =>
        page.evaluate(async () => {
          const found = (await navigator.serviceWorker.getRegistration())!;
          return {
            waiting: found.waiting !== null,
            active: found.active?.state,
          };
        }),
      )
      .toEqual({ waiting: false, active: "activated" });
  } finally {
    await proxy.close();
  }
});

test("outside a secure context no worker is registered and the app still loads", async ({
  page,
  baseURL,
}) => {
  const address = Object.values(networkInterfaces())
    .flat()
    .find((entry) => entry && entry.family === "IPv4" && !entry.internal);
  test.skip(!address, "The browser container has no non-loopback address");
  const insecure = `http://${address!.address}:${new URL(baseURL!).port}`;
  await page.goto(`${insecure}/`);
  await expect(
    page.getByRole("heading", { name: "Sign in", exact: true }),
  ).toBeVisible();
  const state = await page.evaluate(() => ({
    secure: window.isSecureContext,
    available: "serviceWorker" in navigator,
  }));
  expect(state).toEqual({ secure: false, available: false });
});
