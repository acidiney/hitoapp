import { expect, test, type Page } from "@playwright/test";
import type { SyncBatch } from "../src/shared/types";
import { openLibrarySection } from "./fixtures/library-sections";

// Requires a fresh e2e-server with scripts/audio-fixtures.mjs --e2e fixtures.
// Its configured origins must include both test URLs in the browser namespace.
const lanOrigin = process.env.HITO_TEST_URL ?? "http://e2e-server:8080";
const localhostOrigin =
  process.env.HITO_TEST_LOCALHOST_URL ?? "http://localhost:8080";
const admin = "lan-admin";
const adminPassword = "owned LAN HTTP administrator password";
const listener = "lan-listener";
const listenerPassword = "owned LAN HTTP listener password";
const privatePlaylist = "Private LAN HTTP favorites";
const uuidV4 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

declare global {
  interface Window {
    __hitoLanHistoryWrite: (
      method: string,
      privateData: boolean,
    ) => Promise<void>;
    __hitoLanHistoryPrivate: (value: unknown) => boolean;
  }
}

test.use({ launchOptions: { args: [] } });

if (process.env.HITO_TEST_MOBILE === "1")
  test.use({ viewport: { width: 390, height: 844 } });

async function signIn(page: Page, username: string, password: string) {
  await expect(
    page.getByRole("heading", { name: "Welcome back" }),
  ).toBeVisible();
  await page.getByLabel("Username", { exact: true }).fill(username);
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
  await expect(page.locator(".account")).toContainText(username);
}

async function checkHistory(page: Page, encrypted: boolean) {
  await expect
    .poll(() =>
      page.evaluate(() => {
        const audit = window;
        const entry: unknown = window.history.state?.page;
        return {
          privateData: audit.__hitoLanHistoryPrivate(window.history.state),
          encrypted: entry instanceof ArrayBuffer && entry.byteLength > 28,
        };
      }),
    )
    .toEqual({ privateData: false, encrypted });
}

test("LAN HTTP first-run, private navigation, mutations, playback, and localhost login", async ({
  page,
  context,
  request,
}) => {
  test.skip(
    process.env.HITO_LAN_HTTP_E2E !== "1",
    "Run alone against the disposable LAN HTTP first-run server",
  );
  test.setTimeout(120_000);

  const lan = new URL(lanOrigin);
  const localhost = new URL(localhostOrigin);
  expect(lan.protocol).toBe("http:");
  expect(lan.hostname).not.toMatch(
    /^(localhost|.*\.localhost|127\..*|\[::1\]|0\.0\.0\.0)$/i,
  );
  expect(localhost.protocol).toBe("http:");
  expect(localhost.hostname).toBe("localhost");
  expect(localhost.origin).not.toBe(lan.origin);

  // Refuse initialized servers before typing any owned fixture credentials.
  const setup = await request.get(new URL("/api/v1/setup", lan).href);
  expect(setup.status()).toBe(200);
  expect(await setup.json()).toEqual({ needs_setup: true });

  const errors: string[] = [];
  const privateHistoryWrites: string[] = [];
  const observedPages = new Set<Page>();
  const observe = (observed: Page) => {
    if (observedPages.has(observed)) return;
    observedPages.add(observed);
    observed.on("pageerror", (error) => errors.push(error.message));
  };
  context.on("page", observe);
  observe(page);
  await context.exposeBinding(
    "__hitoLanHistoryWrite",
    (_source, method: string, privateData: boolean) => {
      if (privateData) privateHistoryWrites.push(method);
    },
  );
  await context.addInitScript(() => {
    const audit = window;
    const containsPrivate = (value: unknown): boolean => {
      if (typeof value === "string") {
        try {
          return containsPrivate(JSON.parse(value));
        } catch {
          return false;
        }
      }
      if (!value || typeof value !== "object" || value instanceof ArrayBuffer)
        return false;
      if ("props" in value && value.props && typeof value.props === "object") {
        if (
          Object.keys(value.props).some(
            (key) => key !== "errors" && key !== "needs_setup",
          )
        )
          return true;
      }
      return Object.values(value).some(containsPrivate);
    };
    audit.__hitoLanHistoryPrivate = containsPrivate;
    for (const method of ["pushState", "replaceState"] as const) {
      const original = window.history[method].bind(window.history);
      window.history[method] = (data, unused, url) => {
        original(data, unused, url);
        void audit.__hitoLanHistoryWrite(method, containsPrivate(data));
      };
    }
  });

  try {
    await page.goto(new URL("/", lan).href);
    expect(
      await page.evaluate(() => ({
        secure: window.isSecureContext,
        randomUUID: typeof crypto.randomUUID,
        subtle: typeof crypto.subtle,
        randomBytes: crypto.getRandomValues(new Uint8Array(16)).byteLength,
      })),
    ).toEqual({
      secure: false,
      randomUUID: "undefined",
      subtle: "undefined",
      randomBytes: 16,
    });
    await expect(
      page.getByRole("heading", { name: "Set up Hito" }),
    ).toBeVisible();
    await page.getByLabel("Username", { exact: true }).fill(admin);
    await page.getByLabel("Password", { exact: true }).fill(adminPassword);
    await page.getByRole("button", { name: "Create administrator" }).click();
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    await checkHistory(page, false);

    const navigation = page.getByRole("navigation", {
      name: "Main navigation",
    });
    await page
      .getByRole("link", { name: "Account and settings", exact: true })
      .click();
    for (const folder of ["Music", "Archive"]) {
      await page.getByRole("button", { name: "Add music folder" }).click();
      const picker = page.getByRole("dialog", { name: "Add a music folder" });
      await picker
        .getByRole("button", { name: `Open ${folder}`, exact: true })
        .click();
      await expect(
        picker.getByText("This folder has no subfolders."),
      ).toBeVisible();
      await picker.getByRole("button", { name: "Use this folder" }).click();
      await expect(picker).not.toBeVisible();
    }
    await expect(
      page
        .getByRole("list", { name: "Connected music folders" })
        .getByRole("listitem")
        .filter({ hasText: "1 file read" }),
    ).toHaveCount(2, { timeout: 20_000 });
    await page
      .getByRole("navigation", { name: "Settings sections" })
      .getByRole("link", { name: "Accounts", exact: true })
      .click();
    await page
      .getByRole("button", { name: "Create account", exact: true })
      .click();
    const dialog = page.getByRole("dialog", { name: "Create account" });
    await dialog.getByLabel("Username", { exact: true }).fill(listener);
    await dialog.getByLabel("Password", { exact: true }).fill(listenerPassword);
    await dialog
      .getByRole("button", { name: "Create account", exact: true })
      .click();
    await expect(
      page
        .getByRole("region", { name: "Accounts and access" })
        .getByText(listener, { exact: true }),
    ).toBeVisible();

    await navigation
      .getByRole("link", { name: "Library", exact: true })
      .click();
    await expect(page.getByText("2 tracks", { exact: true })).toBeVisible();
    await page
      .getByRole("button", { name: "Play Fixture 440 Hz", exact: true })
      .click();
    await expect
      .poll(() =>
        page
          .locator("audio")
          .evaluate((audio: HTMLAudioElement) => audio.currentTime),
      )
      .toBeGreaterThan(0.2);
    const source = await page
      .locator("audio")
      .evaluate((audio: HTMLAudioElement) => audio.src);
    const before = await page
      .locator("audio")
      .evaluate((audio: HTMLAudioElement) => audio.currentTime);
    await openLibrarySection(page, "Albums");
    await expect(
      page.getByRole("heading", { name: "Albums", exact: true }),
    ).toBeVisible();
    await expect
      .poll(() =>
        page
          .locator("audio")
          .evaluate((audio: HTMLAudioElement) => audio.currentTime),
      )
      .toBeGreaterThan(before);
    expect(
      await page
        .locator("audio")
        .evaluate((audio: HTMLAudioElement) => audio.src),
    ).toBe(source);
    await checkHistory(page, false);
    await page
      .getByRole("button", { name: "Expand player", exact: true })
      .click();
    const queue = page.getByRole("dialog", {
      name: "Now playing",
      exact: true,
    });
    await expect(queue.locator(".queue-tracks li")).toHaveCount(2);
    await queue.getByRole("button", { name: "Close now playing" }).click();

    await openLibrarySection(page, "Playlists");
    await page
      .getByLabel("New playlist", { exact: true })
      .fill(privatePlaylist);
    const mutation = page.waitForRequest((outgoing) => {
      if (
        outgoing.method() !== "POST" ||
        new URL(outgoing.url()).pathname !== "/api/v1/sync"
      )
        return false;
      const batch = outgoing.postDataJSON() as SyncBatch;
      return batch.operations.some(
        (operation) => operation.action.kind === "playlist_create",
      );
    });
    await page
      .getByRole("button", { name: "Create playlist", exact: true })
      .click();
    const batch = (await mutation).postDataJSON() as SyncBatch;
    expect(batch.device_id).toMatch(uuidV4);
    expect(batch.operations).toHaveLength(1);
    expect(batch.operations[0].id).toMatch(uuidV4);
    const action = batch.operations[0].action;
    expect(action.kind).toBe("playlist_create");
    if (action.kind === "playlist_create")
      expect(action.playlist_id).toMatch(uuidV4);
    await expect(
      page
        .locator(".playlist-list")
        .getByText(privatePlaylist, { exact: true }),
    ).toBeVisible();
    await checkHistory(page, false);
    await page.getByRole("button", { name: "Pause", exact: true }).click();
    await page.getByRole("button", { name: "Sign out", exact: true }).click();
    await signIn(page, listener, listenerPassword);
    await expect(page.getByText("0 tracks", { exact: true })).toBeVisible();
    await expect(page.getByText(privatePlaylist, { exact: true })).toHaveCount(
      0,
    );
    await page.goBack();
    await expect(page.locator(".account")).toContainText(listener);
    await expect(page.getByText(privatePlaylist, { exact: true })).toHaveCount(
      0,
    );
    await expect(page.locator("audio")).toHaveCount(1);
    expect(
      await page
        .locator("audio")
        .evaluate((audio: HTMLAudioElement) => audio.paused),
    ).toBe(true);
    expect(await page.locator("audio").getAttribute("src")).toBeNull();
    await checkHistory(page, false);
    await page.getByRole("button", { name: "Sign out", exact: true }).click();
    await signIn(page, admin, adminPassword);
    if (process.env.HITO_TEST_MOBILE === "1") {
      await openLibrarySection(page, "Playlists");
      await expect(
        page
          .locator(".playlist-list")
          .getByText(privatePlaylist, { exact: true }),
      ).toBeVisible();
    } else {
      await expect(
        page
          .locator(".sidebar-playlists")
          .getByText(privatePlaylist, { exact: true }),
      ).toBeVisible();
    }

    const localPage = await context.newPage();
    await localPage.goto(new URL("/", localhost).href);
    expect(
      await localPage.evaluate(() => ({
        secure: window.isSecureContext,
        randomUUID: typeof crypto.randomUUID,
        subtle: typeof crypto.subtle,
      })),
    ).toEqual({ secure: true, randomUUID: "function", subtle: "object" });
    await signIn(localPage, admin, adminPassword);
    await openLibrarySection(localPage, "Playlists");
    await expect(
      localPage
        .locator(".playlist-list")
        .getByText(privatePlaylist, { exact: true }),
    ).toBeVisible();
    await checkHistory(localPage, true);
    await localPage
      .getByRole("button", { name: "Sign out", exact: true })
      .click();
    await expect(
      localPage.getByRole("heading", { name: "Welcome back" }),
    ).toBeVisible();
    await localPage.close();
  } finally {
    if (!page.isClosed()) {
      await page
        .evaluate(async () => {
          const record = window.__hitoLanHistoryWrite;
          if (typeof record === "function") await record("checkpoint", false);
        })
        .catch(() => {});
    }
    expect
      .soft(
        privateHistoryWrites,
        "Private page props must never be persisted in raw browser history",
      )
      .toEqual([]);
    expect
      .soft(
        errors.filter((error) =>
          /randomUUID.*(?:not a function|undefined)|TypeError.*randomUUID/i.test(
            error,
          ),
        ),
        "No randomUUID TypeError",
      )
      .toEqual([]);
    expect.soft(errors, "No uncaught browser errors").toEqual([]);
  }
});
