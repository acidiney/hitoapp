import { isDeepStrictEqual } from "node:util";
import { expect, type Page } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import { libraryId, type LibraryFixture } from "./audiobook-library-state";
import type { LibraryDelivery } from "./audiobook-library-delivery";
import { openLibrarySection } from "./library-sections";
import { networkQuiet } from "./settled";

export async function documentToken(page: Page) {
  return page.evaluate(
    () =>
      (globalThis as unknown as { __hitoLibraryDocument: string })
        .__hitoLibraryDocument,
  );
}
export async function assertBooks(page: Page, books: Schema<"Playlist">[]) {
  await expect(
    page
      .locator("main")
      .getByRole("heading", { name: "Audiobooks", level: 1, exact: true }),
  ).toBeVisible();
  const links = page.locator("main a[href^='/audiobooks/']");
  await expect(links).toHaveCount(books.length);
  await expect
    .poll(async () =>
      isDeepStrictEqual(
        await links.evaluateAll((nodes) =>
          nodes.map((node) => node.getAttribute("href")),
        ),
        books.map((book) => `/audiobooks/${book.id}`),
      ),
    )
    .toBe(true);
  await expect
    .poll(async () =>
      isDeepStrictEqual(
        await links.locator("strong").allTextContents(),
        books.map((book) => book.name),
      ),
    )
    .toBe(true);
}
export async function pageLink(
  page: Page,
  name: string,
  books: Schema<"Playlist">[],
  delivery: LibraryDelivery,
) {
  const before = delivery.counts.visits;
  const token = await documentToken(page);
  const link = page
    .getByRole("navigation", { name: "Audiobook pages", exact: true })
    .getByRole("link", { name, exact: true });
  const href = await link.getAttribute("href");
  if (!href) throw new Error("The owned book page link has no address");
  await link.click();
  await assertBooks(page, books);
  expect(delivery.counts.visits - before).toBe(1);
  expect(await documentToken(page)).toBe(token);
  await networkQuiet(page);
  return href;
}
export async function openBooks(
  page: Page,
  fixture: LibraryFixture,
  delivery: LibraryDelivery,
) {
  const token = await documentToken(page);
  await openLibrarySection(page, "Audiobooks");
  await assertBooks(page, fixture.books.slice(0, 60));
  expect(await documentToken(page)).toBe(token);
  expect(delivery.counts.visits).toBe(1);
  await page
    .getByRole("button", { name: "Add audiobook from a playlist", exact: true })
    .click();
  const options = await page
    .getByRole("combobox", { name: "Playlist", exact: true })
    .locator("option")
    .evaluateAll((nodes) =>
      nodes.map((node) => (node as HTMLOptionElement).value),
    );
  expect(options.includes(fixture.saved.playlist)).toBe(true);
  expect(
    options.some(
      (id) =>
        fixture.books.some((book) => book.id === id) ||
        id === fixture.deleted ||
        id === fixture.peerBook,
    ),
  ).toBe(false);
  await expect(
    page
      .locator("main")
      .getByRole("link", { name: fixture.peerName, exact: true }),
  ).toHaveCount(0);
  await page.keyboard.press("Escape");
  await networkQuiet(page);
}

export async function traverseLibrary(
  page: Page,
  fixture: LibraryFixture,
  delivery: LibraryDelivery,
) {
  await openBooks(page, fixture, delivery);
  await pageLink(
    page,
    "Next audiobooks",
    fixture.books.slice(60, 120),
    delivery,
  );
  await pageLink(page, "Next audiobooks", fixture.books.slice(120), delivery);
  await expect(
    page.getByRole("link", { name: "Next audiobooks", exact: true }),
  ).toHaveCount(0);
  await pageLink(
    page,
    "Previous audiobooks",
    fixture.books.slice(60, 120),
    delivery,
  );
  await pageLink(
    page,
    "First audiobooks",
    fixture.books.slice(0, 60),
    delivery,
  );
  const href = await pageLink(
    page,
    "Next audiobooks",
    fixture.books.slice(60, 120),
    delivery,
  );
  const token = await documentToken(page);
  await page.reload();
  await assertBooks(page, fixture.books.slice(60, 120));
  expect(new URL(page.url()).pathname + new URL(page.url()).search).toBe(href);
  const restored = await documentToken(page);
  expect(restored !== token).toBe(true);
  await networkQuiet(page);
  await page.goBack();
  await assertBooks(page, fixture.books.slice(0, 60));
  expect(await documentToken(page)).toBe(restored);
  await page.goForward();
  await assertBooks(page, fixture.books.slice(60, 120));
  expect(await documentToken(page)).toBe(restored);
  await pageLink(
    page,
    "First audiobooks",
    fixture.books.slice(0, 60),
    delivery,
  );
}

export async function controlNameCursors(fixture: LibraryFixture) {
  const controls = [
    { id: libraryId(1001), name: "\u0001".repeat(256), kind: "local" as const },
    { id: libraryId(1002), name: "\u0001".repeat(256), kind: "local" as const },
  ];
  fixture.remember(controls[0].name);
  const operations = await fixture.owner.sync(
    controls.flatMap((book) => [
      {
        kind: "playlist_create" as const,
        playlist_id: book.id,
        name: book.name,
      },
      { kind: "audiobook_create" as const, playlist_id: book.id },
    ]),
  );
  const first = await fixture.owner.read<Schema<"AudiobookLibraryPage">>(
    "/api/v1/audiobooks/page?limit=1",
  );
  expect(isDeepStrictEqual(first.items, [controls[0]])).toBe(true);
  const next = first.pagination.next_cursor;
  if (!next || next.length <= 2048 || next.length > 4096)
    throw new Error(
      "The owned escaped-name cursor did not exercise its byte bound",
    );
  fixture.remember(next);
  const second = await fixture.owner.read<Schema<"AudiobookLibraryPage">>(
    `/api/v1/audiobooks/page?limit=1&cursor=${encodeURIComponent(next)}`,
  );
  expect(isDeepStrictEqual(second.items, [controls[1]])).toBe(true);
  const previous = second.pagination.previous_cursor;
  if (!previous)
    throw new Error("The owned escaped-name previous cursor is missing");
  fixture.remember(previous);
  const back = await fixture.owner.read<Schema<"AudiobookLibraryPage">>(
    `/api/v1/audiobooks/page?limit=1&cursor=${encodeURIComponent(previous)}`,
  );
  expect(isDeepStrictEqual(back.items, first.items)).toBe(true);
  return { controls, operations };
}
