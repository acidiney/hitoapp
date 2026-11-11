import { expect, test, type Page } from "@playwright/test";
import { createPlaylist } from "./fixtures/playlist-dialogs";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";
import {
  addToPlaylistFromRow,
  openTrackMenu,
  trackTitleLink,
} from "./fixtures/track-row";

const first = "Fixture 440 Hz";
const second = "Fixture 660 Hz";
const playlistName = "Row menu list";

async function openLibraryList(page: Page) {
  await page.goto("/library?q=Hz&sort=title");
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Grid view", exact: true }).click();
  const grid = page.locator(".track-grid .track-card");
  if ((await grid.filter({ hasText: second }).count()) === 0)
    throw new Error("The scanned fixture library is required");
  await page.getByRole("button", { name: "List view", exact: true }).click();
  const table = page.getByRole("table", { name: "Tracks", exact: true });
  await expect(
    table.locator("tbody tr").filter({ hasText: second }),
  ).toHaveCount(1);
  return table;
}

async function openQueue(page: Page) {
  await page
    .getByRole("contentinfo", { name: "Audio player", exact: true })
    .getByRole("button", { name: "Open queue", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Queue", exact: true, level: 1 }),
  ).toBeVisible();
}

test("the row menu opens from the keyboard, plays next, queues, adds to a playlist and links its names", async ({
  page,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));

  const table = await openLibraryList(page);
  const row = table.locator("tbody tr").filter({ hasText: second });
  await expect(row.locator("select")).toHaveCount(0);
  const trigger = row.getByRole("button", {
    name: `Actions for ${second}`,
    exact: true,
  });
  await trigger.focus();
  await page.keyboard.press("Enter");
  const menu = page.getByRole("menu", { name: `Actions for ${second}` });
  await expect(menu).toBeVisible();
  await expect(menu.getByRole("menuitem")).toHaveText([
    "Track details",
    "Play next",
    "Add to queue",
    "Add to playlist",
    "Fix source",
  ]);
  await expect(
    menu.getByRole("menuitem", { name: "Track details" }),
  ).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(menu.getByRole("menuitem", { name: "Play next" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  await expect(trigger).toBeFocused();

  const firstRow = table.locator("tbody tr").filter({ hasText: first });
  await row
    .getByRole("button", { name: `Play ${second}`, exact: true })
    .click();
  await expect
    .poll(() =>
      page
        .locator("audio[data-hito-player]")
        .evaluate((audio: HTMLAudioElement) => audio.currentTime),
    )
    .toBeGreaterThan(0.2);
  await (await openTrackMenu(firstRow, first))
    .getByRole("menuitem", { name: "Play next" })
    .click();
  await (await openTrackMenu(row, second))
    .getByRole("menuitem", { name: "Add to queue" })
    .click();
  await openQueue(page);
  await expect(page.locator(".queue-now-row strong")).toHaveText(second);
  await expect(page.locator(".queue-next .queue-track strong")).toHaveText([
    first,
    second,
  ]);

  await page.goto("/playlists");
  const existing = page.locator(".playlist-list").getByText(playlistName);
  if ((await existing.count()) === 0) {
    await createPlaylist(page, playlistName);
    await expect(
      page.locator(".playlist-list").getByText(playlistName),
    ).toBeVisible();
  }
  const list = await openLibraryList(page);
  await addToPlaylistFromRow(
    list.locator("tbody tr").filter({ hasText: second }),
    second,
    playlistName,
  );
  await expect(
    page.getByRole("status").filter({ hasText: "Added to" }),
  ).toContainText(`Added to ${playlistName}`);
  await page
    .locator(".sidebar-playlists")
    .getByRole("link", { name: playlistName })
    .click();
  await expect(
    page.getByRole("heading", { name: playlistName, exact: true }),
  ).toBeVisible();
  const entries = page.getByRole("table", {
    name: "Playlist entries",
    exact: true,
  });
  await expect(
    entries.locator("tbody tr").filter({ hasText: second }),
  ).toHaveCount(1);
  await expect(entries.locator("thead th")).toHaveText([
    /Play/,
    "Title",
    "Album",
    "Availability",
    "Time",
    /Actions/,
  ]);
  await entries
    .locator("tbody tr")
    .filter({ hasText: second })
    .locator('td.album-column a[href^="/albums/"]')
    .click();
  await expect(page).toHaveURL(/\/albums\/[^/]+$/);
  await expect(
    page.getByRole("heading", {
      name: "Fixture collection 660 Hz",
      exact: true,
    }),
  ).toBeVisible();

  const listed = await openLibraryList(page);
  await listed
    .locator("tbody tr")
    .filter({ hasText: second })
    .locator('.track-meta a[href^="/artists/"]')
    .click();
  await expect(page).toHaveURL(/\/artists\/[^/]+$/);
  await expect(page.locator("main")).toContainText(second);

  const library = await openLibraryList(page);
  const albumLink = library
    .locator("tbody tr")
    .filter({ hasText: second })
    .getByRole("link", { name: `Fixture collection 660 Hz`, exact: true })
    .first();
  await albumLink.click();
  await expect(page).toHaveURL(/\/albums\/[^/]+$/);
  await expect(
    page.getByRole("heading", {
      name: "Fixture collection 660 Hz",
      exact: true,
    }),
  ).toBeVisible();
  const albumTable = page.getByRole("table", { name: "Tracks", exact: true });
  await expect(albumTable.locator("thead th")).toHaveText([
    /Play/,
    "Title",
    "Time",
    /Actions/,
  ]);
  await trackTitleLink(albumTable, second).click();
  await expect(page).toHaveURL(/\/recordings\/[^/]+$/);
  await expect(
    page.getByRole("heading", { name: second, exact: true, level: 1 }),
  ).toBeVisible();

  await page.setViewportSize({ width: 390, height: 844 });
  const phoneTable = await openLibraryList(page);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
  ).toBe(false);
  const phoneMenu = await openTrackMenu(
    phoneTable.locator("tbody tr").filter({ hasText: second }),
    second,
  );
  const box = await phoneMenu.boundingBox();
  expect(box).not.toBeNull();
  if (box) {
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(390);
  }
  await page.keyboard.press("Escape");
  expect(errors).toEqual([]);
});
