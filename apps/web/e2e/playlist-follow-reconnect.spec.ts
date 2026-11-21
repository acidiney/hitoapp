import { openAllManage } from "./fixtures/plugin-ui";
import { expect, test, type Page } from "@playwright/test";
import { openPlaylistImport } from "./fixtures/playlist-dialogs";
import type { Schema } from "../src/shared/schema";
import { playlistFollowFixture } from "./fixtures/playlist-follow";
import { reviewPluginInstallation } from "./fixtures/plugin-installation-review";
import {
  connectLiveProvider,
  setUpSpotifyYouTubeLiveAccount as setUpDisposableFollowAccount,
} from "./fixtures/spotify-youtube-live";
import { openLibrarySection } from "./fixtures/library-sections";

test.use({ trace: "off", screenshot: "off", video: "off" });

test.describe("isolated provider playlist reconnect", () => {
  test.skip(
    process.env.HITO_PLAYLIST_FOLLOW_E2E !== "1",
    "Requires explicit disposable Follow server opt-in",
  );

  test("a removed connection offers to connect again and a reconnect keeps the Follow updating", async ({
    page,
  }, testInfo) => {
    await page.emulateMedia({ colorScheme: "dark" });
    const fixture = await playlistFollowFixture();
    const services: Schema<"PluginService">[] = [];
    const capture = async (name: string, target = approval(page)) => {
      await page.setViewportSize({ width: 1440, height: 1000 });
      await page.screenshot({
        path: testInfo.outputPath(`${name}-desktop.png`),
      });
      await page.setViewportSize({ width: 390, height: 844 });
      if (await target.count()) await target.scrollIntoViewIfNeeded();
      await page.screenshot({ path: testInfo.outputPath(`${name}-390.png`) });
      expect(
        await page.evaluate(
          () =>
            document.documentElement.scrollWidth <=
            document.documentElement.clientWidth,
        ),
      ).toBe(true);
      await page.setViewportSize({ width: 1440, height: 1000 });
    };
    try {
      await setUpDisposableFollowAccount(page);
      let connection = await connectLiveProvider(
        page,
        fixture.address,
        fixture.manifest.name,
        (service) => services.push(service),
      );
      const [offered] = services;
      await openLibrarySection(page, "Playlists");
      await openPlaylistImport(page);
      const form = page.getByRole("region", {
        name: "Import a playlist",
        exact: true,
      });
      await form.getByRole("radio", { name: /^Follow/ }).check();
      await form
        .getByRole("textbox", { name: "Playlist link or identifier" })
        .fill(fixture.reference);
      const followed = page.waitForResponse(
        (response) =>
          new URL(response.url()).pathname ===
            `/api/v1/plugins/${connection.id}/playlists/follow` &&
          response.request().method() === "POST",
      );
      await form
        .getByRole("button", { name: "Follow playlist", exact: true })
        .click();
      const receipt: Schema<"PlaylistFollowReceipt"> = await (
        await followed
      ).json();
      const playlistPath = `/playlists/${receipt.playlist_id}`;
      const statusPath = `/api/v1/playlists/${receipt.playlist_id}/follow`;
      await expect.poll(() => new URL(page.url()).pathname).toBe(playlistPath);
      await expect(
        updates(page).getByText("Ready", { exact: true }),
      ).toBeVisible();

      await remove(page, connection);
      await page.goto(playlistPath);
      await expect(
        updates(page).getByText("Paused", { exact: true }),
      ).toBeVisible();
      await expect(
        approval(page).getByText(
          `The ${fixture.manifest.name} connection was removed or replaced.`,
          { exact: true },
        ),
      ).toBeVisible();
      const again = approval(page).getByRole("link", {
        name: `Connect ${fixture.manifest.name} again`,
        exact: true,
      });
      await expect(again).toBeVisible();
      await expect(
        approval(page).getByRole("button", { name: "Resume updates" }),
      ).toHaveCount(0);
      await capture("follow-removed");

      await again.click();
      const dialog = page.getByRole("dialog", {
        name: `Connect ${offered.name}`,
        exact: true,
      });
      await expect(dialog).toBeVisible();
      await expect
        .poll(() => new URL(page.url()).search)
        .toBe("?section=plugins");
      await capture("follow-connect-again", dialog);
      connection = await accept(page, offered, fixture.manifest.name);
      await page.goto(playlistPath);
      await expect(approval(page)).toHaveCount(0);
      await expect(
        updates(page).getByText("Paused", { exact: true }),
      ).toHaveCount(0);
      const carried = await status(page, statusPath);
      expect(carried.status).not.toBe("paused");
      expect(carried.review ?? null).toBeNull();
      await capture("follow-reconnected", updates(page));

      await remove(page, connection);
      const widened = fixture.reviewManifest();
      await page.goto("/settings?section=plugins");
      await page
        .getByRole("button", { name: `Connect ${offered.name}`, exact: true })
        .click();
      await accept(page, offered, widened.name);
      await page.goto(playlistPath);
      await expect(
        updates(page).getByText("Paused", { exact: true }),
      ).toBeVisible();
      await expect(
        approval(page).getByText(
          `The ${widened.name} connection now also offers song search.`,
          { exact: true },
        ),
      ).toBeVisible();
      const resume = approval(page).getByRole("button", {
        name: "Resume updates",
        exact: true,
      });
      await expect(resume).toBeEnabled();
      await capture("follow-reconnected-review");
      await resume.click();
      await expect(approval(page)).toHaveCount(0);
      expect((await status(page, statusPath)).status).not.toBe("paused");
    } finally {
      await fixture.close();
    }
  });
});

function updates(page: Page) {
  return page.getByRole("region", { name: "Playlist updates", exact: true });
}

function approval(page: Page) {
  return page.getByRole("region", {
    name: "Resume playlist updates",
    exact: true,
  });
}

async function remove(page: Page, connection: Schema<"PluginInstallation">) {
  await page.goto("/settings?section=plugins");
  await openAllManage(page);
  await page
    .locator(".plugin-connection")
    .first()
    .getByRole("button", { name: "Remove", exact: true })
    .click();
  const removed = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname === `/api/v1/plugins/${connection.id}` &&
      response.request().method() === "DELETE",
  );
  await page
    .getByRole("dialog", { name: `Remove ${connection.manifest.name}?` })
    .getByRole("button", { name: "Remove", exact: true })
    .click();
  expect((await removed).status()).toBe(204);
}

async function accept(
  page: Page,
  service: Schema<"PluginService">,
  name: string,
) {
  await reviewPluginInstallation(page, service, name);
  const installed = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname === "/api/v1/plugins" &&
      response.request().method() === "POST",
  );
  await page
    .getByRole("button", { name: "Accept and connect", exact: true })
    .click();
  const response = await installed;
  expect(response.status()).toBe(201);
  const connection: Schema<"PluginInstallation"> = await response.json();
  return connection;
}

async function status(page: Page, path: string) {
  const response = await page.request.get(path);
  expect(response.status()).toBe(200);
  const body: Schema<"PlaylistFollow"> = await response.json();
  return body;
}
