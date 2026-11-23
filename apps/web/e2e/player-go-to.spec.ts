import { expect, test, type Page } from "@playwright/test";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";
import { openTrackMenu } from "./fixtures/track-row";

const title = "Fixture 440 Hz";
const album = "Fixture collection 440 Hz";
const artist = "Hito test fixtures";

function dock(page: Page) {
  return page.getByRole("contentinfo", { name: "Audio player", exact: true });
}

function nowPlaying(page: Page) {
  return page.getByRole("dialog", { name: "Now playing" });
}

async function playFixture(page: Page) {
  await page.goto("/library?q=Hz");
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "List view", exact: true }).click();
  await page
    .getByRole("table", { name: "Tracks", exact: true })
    .locator("tbody tr")
    .filter({ hasText: title })
    .getByRole("button", { name: `Play ${title}`, exact: true })
    .click();
  await expect(dock(page).getByRole("link", { name: title })).toBeVisible();
}

async function openNowPlaying(page: Page, phone = false) {
  await dock(page)
    .getByRole("button", {
      name: phone ? "Expand player" : "Open now playing",
      exact: true,
    })
    .click();
  await expect(nowPlaying(page)).toBeVisible();
}

async function expectPage(page: Page, heading: string) {
  await expect(nowPlaying(page)).toHaveCount(0);
  await expect(
    page.getByRole("heading", { name: heading, exact: true, level: 1 }),
  ).toBeVisible();
}

test("the player goes to the playing track's album and artist and closes Now playing first", async ({
  page,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await playFixture(page);

  await openNowPlaying(page);
  await nowPlaying(page)
    .getByRole("link", { name: title, exact: true })
    .click();
  await expect(page).toHaveURL(/\/albums\/[^/?#]+$/);
  await expectPage(page, album);

  await openNowPlaying(page);
  const menu = await openTrackMenu(nowPlaying(page), title);
  await menu.getByRole("menuitem", { name: "Go to artist" }).click();
  await expect(page).toHaveURL(/\/artists\/[^/?#]+$/);
  await expectPage(page, artist);

  await page.goto("/library");
  const bar = await openTrackMenu(dock(page), title);
  await bar.getByRole("menuitem", { name: "Go to album" }).click();
  await expectPage(page, album);
  expect(errors).toEqual([]);
});

test("on a phone, Now playing's menu goes to the album and closes the full-screen player", async ({
  page,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  await page.setViewportSize({ width: 390, height: 844 });
  await playFixture(page);
  await openNowPlaying(page, true);
  const menu = await openTrackMenu(nowPlaying(page), title);
  await expect(menu.getByRole("menuitem")).toHaveText([
    "Go to artist",
    "Go to album",
    "Track details",
    "Add to playlist",
    "Fix source",
  ]);
  await menu.getByRole("menuitem", { name: "Go to album" }).click();
  await expectPage(page, album);
});
