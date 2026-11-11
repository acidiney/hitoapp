import { randomUUID } from "node:crypto";
import { expect, test, type Page } from "@playwright/test";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";
import { createPlaylist } from "./fixtures/playlist-dialogs";
import { enterEditMode } from "./fixtures/playlist-edit";
import { addToPlaylistFromRow } from "./fixtures/track-row";

const first = "Fixture 440 Hz";
const second = "Fixture 660 Hz";

async function addFixtureTracks(page: Page, playlist: string) {
  await page.goto("/library?q=Hz");
  await page.getByRole("button", { name: "List view", exact: true }).click();
  const table = page.getByRole("table", { name: "Tracks", exact: true });
  for (const title of [first, second]) {
    await addToPlaylistFromRow(
      table.locator("tbody tr").filter({ hasText: title }),
      title,
      playlist,
    );
  }
}

function entryTitles(page: Page) {
  return page
    .getByRole("table", { name: "Playlist entries", exact: true })
    .locator("tbody tr .track-title")
    .allTextContents();
}

test("Edit stages the name, order and removals; Cancel reverts them and Done saves them in one request", async ({
  page,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const suffix = randomUUID().slice(0, 8);
  const name = `Domingo com um nome bastante longo para testar o corte ${suffix}`;
  const renamed = `Domingo curto ${suffix}`;

  await page.goto("/playlists");
  await createPlaylist(page, name);
  await addFixtureTracks(page, name);

  const link = page.locator(".sidebar-playlists").getByRole("link", { name });
  const label = link.locator("[title]");
  await expect(label).toHaveAttribute("title", name);
  const clipped = await label.evaluate((element) => ({
    overflow: getComputedStyle(element).textOverflow,
    cut: element.scrollWidth > element.clientWidth,
    inside:
      element.getBoundingClientRect().right <=
      (element.closest(".sidebar") as HTMLElement).getBoundingClientRect()
        .right,
  }));
  expect(clipped).toEqual({ overflow: "ellipsis", cut: true, inside: true });

  await link.click();
  await expect(page.getByRole("heading", { name, exact: true })).toBeVisible();
  expect(await entryTitles(page)).toEqual([first, second]);

  const syncs: string[] = [];
  page.on("request", (request) => {
    if (
      new URL(request.url()).pathname === "/api/v1/sync" &&
      request.method() === "POST"
    )
      syncs.push(request.postData() ?? "");
  });

  await enterEditMode(page);
  await expect(
    page.getByRole("button", { name: "Cancel", exact: true }),
  ).toBeVisible();
  await expect(page.getByRole("button", { name: "Save name" })).toHaveCount(0);
  await page.getByRole("textbox", { name: "Name" }).fill(renamed);
  await page
    .getByRole("button", { name: `Move ${first} down`, exact: true })
    .click();
  expect(await entryTitles(page)).toEqual([second, first]);
  await expect(page.getByRole("heading", { name, exact: true })).toBeVisible();
  expect(syncs).toHaveLength(0);

  await page.getByRole("button", { name: "Cancel", exact: true }).click();
  const discard = page.getByRole("dialog", { name: "Discard your changes?" });
  await expect(discard).toBeVisible();
  await discard.getByRole("button", { name: "Keep editing" }).click();
  await expect(page.getByRole("textbox", { name: "Name" })).toHaveValue(
    renamed,
  );
  await page.getByRole("button", { name: "Cancel", exact: true }).click();
  await discard.getByRole("button", { name: "Discard changes" }).click();
  await expect(discard).toBeHidden();
  await expect(
    page.getByRole("button", { name: "Edit", exact: true }),
  ).toBeVisible();
  expect(await entryTitles(page)).toEqual([first, second]);
  await expect(page.getByRole("heading", { name, exact: true })).toBeVisible();
  expect(syncs).toHaveLength(0);

  await enterEditMode(page);
  await expect(page.getByRole("textbox", { name: "Name" })).toHaveValue(name);
  await page.getByRole("textbox", { name: "Name" }).fill(renamed);
  await page
    .getByRole("button", { name: `Move ${first} down`, exact: true })
    .click();
  await page
    .getByRole("button", {
      name: `Remove ${second} from playlist`,
      exact: true,
    })
    .click();
  await expect(
    page.getByRole("button", { name: `Keep ${second} in playlist` }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: `Keep ${second} in playlist`, exact: true })
    .click();
  expect(syncs).toHaveLength(0);

  await page.getByRole("button", { name: "Done", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: renamed, exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Edit", exact: true }),
  ).toBeVisible();
  expect(syncs).toHaveLength(1);
  const operations: { action: { kind: string } }[] = JSON.parse(
    syncs[0],
  ).operations;
  expect(operations.map((operation) => operation.action.kind)).toEqual([
    "playlist_rename",
    "entry_move",
    "entry_move",
  ]);
  await page.reload();
  expect(await entryTitles(page)).toEqual([second, first]);

  await enterEditMode(page);
  await page
    .getByRole("button", {
      name: `Remove ${second} from playlist`,
      exact: true,
    })
    .click();
  await page.getByRole("button", { name: "Done", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Edit", exact: true }),
  ).toBeVisible();
  expect(await entryTitles(page)).toEqual([first]);
  expect(syncs).toHaveLength(2);
  expect(errors).toEqual([]);
});
