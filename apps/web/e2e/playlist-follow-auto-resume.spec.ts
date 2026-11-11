import { openAllManage } from "./fixtures/plugin-ui";
import { expect, test, type Page } from "@playwright/test";
import { openPlaylistImport } from "./fixtures/playlist-dialogs";
import type { Schema } from "../src/shared/schema";
import { playlistFollowFixture } from "./fixtures/playlist-follow";
import {
  connectLiveProvider,
  setUpSpotifyYouTubeLiveAccount as setUpDisposableFollowAccount,
} from "./fixtures/spotify-youtube-live";
import { openLibrarySection } from "./fixtures/library-sections";

test.use({ trace: "off", screenshot: "off", video: "off" });

test.describe("isolated provider playlist connection revisions", () => {
  test.skip(
    process.env.HITO_PLAYLIST_FOLLOW_E2E !== "1",
    "Requires explicit disposable Follow server opt-in",
  );

  test("a rebuilt connection keeps the Follow updating and a gained ability asks before resuming", async ({
    page,
  }, testInfo) => {
    const fixture = await playlistFollowFixture();
    try {
      await setUpDisposableFollowAccount(page);
      const connection = await connectLiveProvider(
        page,
        fixture.address,
        fixture.manifest.name,
        () => {},
      );
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
      await expect
        .poll(() => new URL(page.url()).pathname)
        .toBe(`/playlists/${receipt.playlist_id}`);
      const statusPath = `/api/v1/playlists/${receipt.playlist_id}/follow`;
      const updates = page.getByRole("region", {
        name: "Playlist updates",
        exact: true,
      });
      const approval = page.getByRole("region", {
        name: "Resume playlist updates",
        exact: true,
      });
      await expect(updates.getByText("Ready", { exact: true })).toBeVisible();

      const rebuilt = fixture.rebuildManifest();
      await acceptChanges(page, connection.id, rebuilt.name);
      await page.goto(`/playlists/${receipt.playlist_id}`);
      await expect(approval).toHaveCount(0);
      await expect(updates.getByText("Paused", { exact: true })).toHaveCount(0);
      const resumed = await status(page, statusPath);
      expect(resumed.status).not.toBe("paused");
      expect(resumed.generation).toBe(2);
      expect(resumed.review ?? null).toBeNull();

      const widened = fixture.reviewManifest();
      await acceptChanges(page, connection.id, widened.name);
      await page.goto(`/playlists/${receipt.playlist_id}`);
      await expect(updates.getByText("Paused", { exact: true })).toBeVisible();
      await expect(
        approval.getByText(
          `The ${widened.name} connection now also offers song search.`,
          { exact: true },
        ),
      ).toBeVisible();
      await expect(
        approval.getByText("playlists, search", { exact: true }),
      ).toBeVisible();
      await expect(
        approval.getByRole("button", { name: "Resume updates", exact: true }),
      ).toBeEnabled();
      await expect(
        approval.getByRole("link", { name: "Review connection", exact: true }),
      ).toBeVisible();
      await page.screenshot({
        path: testInfo.outputPath("follow-resume-desktop.png"),
      });
      await page.setViewportSize({ width: 390, height: 844 });
      await approval.scrollIntoViewIfNeeded();
      await expect(approval).toBeInViewport();
      await page.screenshot({
        path: testInfo.outputPath("follow-resume-390.png"),
      });
      expect(
        await page.evaluate(
          () =>
            document.documentElement.scrollWidth <=
            document.documentElement.clientWidth,
        ),
      ).toBe(true);
      await approval
        .getByRole("button", { name: "Resume updates", exact: true })
        .click();
      await expect(approval).toHaveCount(0);
      expect((await status(page, statusPath)).status).not.toBe("paused");
    } finally {
      await fixture.close();
    }
  });
});

async function acceptChanges(page: Page, installation: string, name: string) {
  await page
    .getByRole("link", { name: "Account and settings", exact: true })
    .click();
  await page
    .getByRole("navigation", { name: "Settings sections" })
    .getByRole("link", { name: "Plugins", exact: true })
    .click();
  const card = page.locator(".plugin-connection").first();
  const path = `/api/v1/plugins/${installation}/refresh`;
  const checked = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname === path &&
      response.request().method() === "POST",
  );
  await openAllManage(page);
  await card
    .getByRole("button", { name: "Check connection", exact: true })
    .click();
  expect((await checked).status()).toBe(200);
  await expect(card.getByRole("dialog")).toBeVisible();
  const accepted = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname === path &&
      response.request().method() === "POST",
  );
  await card.getByRole("button", { name: /^Accept / }).click();
  const body: Schema<"PluginRefresh"> = await (await accepted).json();
  expect(body.kind === "updated" && body.installation.manifest.name).toBe(name);
}

async function status(page: Page, path: string) {
  const response = await page.request.get(path);
  expect(response.status()).toBe(200);
  const body: Schema<"PlaylistFollow"> = await response.json();
  return body;
}
