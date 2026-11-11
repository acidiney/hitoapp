import { randomUUID } from "node:crypto";
import { expect, test, type Page } from "@playwright/test";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";
import { createPlaylist } from "./fixtures/playlist-dialogs";
import { connectFixtures } from "./fixtures/streaming-quality";
import { addToPlaylistFromRow } from "./fixtures/track-row";

const titles = ["Fixture 440 Hz", "Fixture 660 Hz"];

async function playlistWithFixtures(page: Page, baseURL: string) {
  if (!sharedAdminSession) await enterAdmin(page);
  await page.goto("/home");
  await connectFixtures(page, baseURL);
  const name = `Now playing ${randomUUID().slice(0, 8)}`;
  await page.goto("/playlists");
  await createPlaylist(page, name);
  await page.goto("/library?q=Hz");
  await page.getByRole("button", { name: "List view", exact: true }).click();
  const table = page.getByRole("table", { name: "Tracks", exact: true });
  for (const title of titles) {
    await addToPlaylistFromRow(
      table.locator("tbody tr").filter({ hasText: title }),
      title,
      name,
    );
  }
  await page.locator(".sidebar-playlists").getByRole("link", { name }).click();
  await expect(page.getByRole("heading", { name, exact: true })).toBeVisible();
  return page.getByRole("table", { name: "Playlist entries", exact: true });
}

test("the playing playlist row shows bars, a link-coloured title and an accessible state", async ({
  page,
  baseURL,
}) => {
  const table = await playlistWithFixtures(page, baseURL!);
  const rows = table.locator("tbody tr");
  await rows
    .nth(1)
    .getByRole("button", { name: /^Play / })
    .click();
  const playing = rows.nth(1);
  await page.mouse.move(0, 0);
  await expect(playing).toHaveAttribute("aria-current", "true");
  await expect(playing.locator(".playing-bars")).toHaveAttribute(
    "data-state",
    "playing",
  );
  await expect(playing.getByText("Now playing")).toBeAttached();
  await expect(rows.nth(0)).not.toHaveAttribute("aria-current", "true");
  const colour = await playing
    .locator(".track-title")
    .evaluate((title) => getComputedStyle(title).color);
  const link = await page.evaluate(() => {
    const probe = document.createElement("a");
    probe.style.color = "var(--link)";
    document.body.append(probe);
    const value = getComputedStyle(probe).color;
    probe.remove();
    return value;
  });
  expect(colour).toBe(link);
  const bars = await playing.locator(".playing-bars").evaluate((element) => {
    const box = element.getBoundingClientRect();
    return { width: box.width, height: box.height };
  });
  expect(bars).toEqual({ width: 18, height: 18 });

  await page
    .getByRole("button", { name: "Pause", exact: true })
    .first()
    .click();
  await expect(playing.locator(".playing-bars")).toHaveAttribute(
    "data-state",
    "paused",
  );
  await expect(playing.getByText("Paused")).toBeAttached();
});

test("the library list marks the playing track", async ({ page, baseURL }) => {
  if (!sharedAdminSession) await enterAdmin(page);
  await page.goto("/home");
  await connectFixtures(page, baseURL!);
  await page.goto("/library?q=Hz");
  await page.getByRole("button", { name: "List view", exact: true }).click();
  const table = page.getByRole("table", { name: "Tracks", exact: true });
  const row = table.locator("tbody tr").filter({ hasText: titles[0] });
  await row.hover();
  await row.getByRole("button", { name: `Play ${titles[0]}` }).click();
  await expect(row).toHaveAttribute("aria-current", "true");
  await expect(row.locator(".playing-bars")).toBeVisible();
});

for (const width of [1440, 1100]) {
  test(`hovering a playlist row keeps Find and the availability text clear of the actions at ${width}px`, async ({
    page,
    baseURL,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    const table = await playlistWithFixtures(page, baseURL!);
    const row = table.locator("tbody tr").first();
    await row.hover();
    const actions = row.locator(".track-actions");
    await expect(actions).toBeVisible();
    const overlap = await row.evaluate((element) => {
      const source = element
        .querySelector(".source-column")!
        .getBoundingClientRect();
      const text = [...element.querySelectorAll(".source-column *")].map(
        (node) => node.getBoundingClientRect(),
      );
      const area = element
        .querySelector(".track-actions")!
        .getBoundingClientRect();
      const farthest = Math.max(source.right, ...text.map((box) => box.right));
      return { clear: farthest <= area.left + 0.5 };
    });
    expect(overlap).toEqual({ clear: true });
    const unsourced = page
      .getByRole("table", { name: "Playlist entries", exact: true })
      .locator(".source-find");
    if ((await unsourced.count()) > 0) {
      const find = unsourced.first();
      const hit = await find.evaluate((link) => {
        const box = link.getBoundingClientRect();
        return (
          document.elementFromPoint(
            box.x + box.width / 2,
            box.y + box.height / 2,
          ) === link
        );
      });
      expect(hit).toBe(true);
    }
  });
}
