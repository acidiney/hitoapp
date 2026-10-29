import { expect, test, type BrowserContext, type Page } from "@playwright/test";
import type { components } from "../src/generated";
import { readSpotifyYouTube } from "./fixtures/spotify-youtube-collection";
import {
  setUpSpotifyYouTubeLiveAccount,
  SpotifyYouTubeLiveFailure,
} from "./fixtures/spotify-youtube-live";
import { signInLiveAccount } from "./fixtures/youtube-live";
import { openLibrarySection } from "./fixtures/library-sections";

type Schema = components["schemas"];

async function ownedSnapshot(page: Page) {
  return {
    user: await readSpotifyYouTube<Schema["User"]>(page, "/api/v1/me"),
    users: await readSpotifyYouTube<Schema["User"][]>(page, "/api/v1/users"),
    playlists: await readSpotifyYouTube<Schema["Playlist"][]>(
      page,
      "/api/v1/playlists",
    ),
    catalog: await readSpotifyYouTube<Schema["RecordingPage"]>(
      page,
      "/api/v1/recordings?limit=1",
    ),
    activity: await readSpotifyYouTube<Schema["Activity"]>(
      page,
      "/api/v1/activity",
    ),
    plugins: await readSpotifyYouTube<Schema["PluginSettings"]>(
      page,
      "/api/v1/plugins",
    ),
  };
}

test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});
test.skip(
  process.env.HITO_SPOTIFY_YOUTUBE_ISOLATION !== "1",
  "Requires an explicitly isolated fresh Hito server",
);

test("the live Spotify to YouTube entry point refuses an initialized server before credentials or POST", async ({
  page,
  browser,
  baseURL,
}) => {
  let phase = 0;
  let guardPosts = 0;
  let guardCredentialInputs = 0;
  let verifier: BrowserContext | undefined;
  try {
    phase = 1;
    if (!baseURL)
      throw new Error("The owned disposable server must be configured");
    const setupResponse = await page.request.get("/api/v1/setup");
    if (setupResponse.status() !== 200)
      throw new Error("The owned fixture setup status could not be checked");
    const setup: Schema["SetupStatus"] = await setupResponse.json();
    if (setup.needs_setup !== true)
      throw new Error(
        "The isolation regression requires an uninitialized disposable server",
      );

    phase = 2;
    await signInLiveAccount(page);
    await openLibrarySection(page, "Playlists");
    await page
      .getByLabel("New playlist", { exact: true })
      .fill("Isolation retained playlist");
    const createdResponse = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === "/api/v1/sync" &&
        response.request().method() === "POST",
    );
    await page
      .getByRole("button", { name: "Create playlist", exact: true })
      .click();
    expect((await createdResponse).status()).toBe(200);
    await expect(
      page
        .locator(".playlist-list")
        .getByText("Isolation retained playlist", { exact: true }),
    ).toBeVisible();

    phase = 3;
    verifier = await browser.newContext({ baseURL });
    const verifierPage = await verifier.newPage();
    await signInLiveAccount(verifierPage);
    const before = await ownedSnapshot(verifierPage);
    expect(before.users.length).toBe(1);
    expect(before.playlists.length).toBe(1);

    phase = 4;
    const logoutResponse = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === "/api/v1/logout" &&
        response.request().method() === "POST",
    );
    await page.getByRole("button", { name: "Sign out", exact: true }).click();
    expect((await logoutResponse).status()).toBe(204);
    await expect(
      page.getByRole("button", { name: "Sign in", exact: true }),
    ).toBeVisible();

    phase = 5;
    page.on("request", (request) => {
      if (request.method() === "POST") guardPosts++;
    });
    await page.exposeFunction("__hitoIsolationCredentialInput", () => {
      guardCredentialInputs++;
    });
    await page.addInitScript(() => {
      document.addEventListener(
        "input",
        (event) => {
          const input = event.target;
          if (
            !(input instanceof HTMLInputElement) ||
            !input.matches(
              'input[autocomplete="username"], input[autocomplete="current-password"], input[autocomplete="new-password"]',
            )
          )
            return;
          const report = (
            window as typeof window & {
              __hitoIsolationCredentialInput: () => Promise<void>;
            }
          ).__hitoIsolationCredentialInput;
          void report().catch(() => {});
        },
        true,
      );
    });
    let refused = false;
    let fixedFailure = false;
    try {
      await setUpSpotifyYouTubeLiveAccount(page);
    } catch (error) {
      refused = true;
      fixedFailure =
        error instanceof SpotifyYouTubeLiveFailure &&
        error.message ===
          "The Spotify to YouTube live test requires an uninitialized disposable server";
    }

    phase = 6;
    const form = await page.evaluate(() => {
      const username = document.querySelector<HTMLInputElement>(
        'input[autocomplete="username"]',
      );
      const password = document.querySelector<HTMLInputElement>(
        'input[type="password"]',
      );
      return {
        present: Number(username !== null && password !== null),
        username_empty: Number(username?.value === ""),
        password_empty: Number(password?.value === ""),
      };
    });
    const after = await ownedSnapshot(verifierPage);
    const accountsPreserved =
      JSON.stringify({ user: before.user, users: before.users }) ===
      JSON.stringify({ user: after.user, users: after.users });
    const collectionsPreserved =
      JSON.stringify({
        playlists: before.playlists,
        catalog: before.catalog,
        activity: before.activity,
        plugins: before.plugins,
      }) ===
      JSON.stringify({
        playlists: after.playlists,
        catalog: after.catalog,
        activity: after.activity,
        plugins: after.plugins,
      });
    console.info(
      JSON.stringify({
        event: "spotify_youtube_isolation_observed",
        refused: Number(refused),
        fixed_failure: Number(fixedFailure),
        guard_posts: guardPosts,
        guard_credential_inputs: guardCredentialInputs,
        form_present: form.present,
        username_empty: form.username_empty,
        password_empty: form.password_empty,
        accounts_preserved: Number(accountsPreserved),
        collections_preserved: Number(collectionsPreserved),
      }),
    );

    phase = 7;
    expect(refused).toBe(true);
    expect(fixedFailure).toBe(true);
    expect(guardPosts).toBe(0);
    expect(guardCredentialInputs).toBe(0);
    expect(form.present).toBe(1);
    expect(form.username_empty + form.password_empty).toBe(2);
    expect(accountsPreserved).toBe(true);
    expect(collectionsPreserved).toBe(true);
  } catch {
    console.info(
      JSON.stringify({
        event: "spotify_youtube_isolation_failed",
        phase,
        guard_posts: guardPosts,
        guard_credential_inputs: guardCredentialInputs,
      }),
    );
    throw new Error("The initialized-server isolation regression failed");
  } finally {
    await verifier?.close().catch(() => {});
    await page.close().catch(() => {});
  }
});
