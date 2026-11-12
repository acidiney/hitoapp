import { signOutFromShell } from "./account";
import { expect, type Page } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import type { LibraryFixture } from "./audiobook-library-state";
import type { LibraryDelivery } from "./audiobook-library-delivery";
import {
  assertBooks,
  documentToken,
  openBooks,
  pageLink,
} from "./audiobook-library-navigation";
import { openLibrarySection } from "./library-sections";
import { networkQuiet } from "./settled";

const navigation = (page: Page) =>
  page.getByRole("navigation", { name: "Audiobook pages", exact: true });
async function nextHref(page: Page) {
  const href = await navigation(page)
    .getByRole("link", { name: "Next audiobooks", exact: true })
    .getAttribute("href");
  if (!href) throw new Error("The owned next-page address is missing");
  return href;
}
async function failedPage(
  page: Page,
  href: string,
  books: Schema<"Playlist">[],
) {
  await expect(
    page
      .getByRole("alert")
      .filter({ hasText: "Hito could not load this audiobook page." }),
  ).toBeVisible({ timeout: 15_000 });
  await assertBooks(page, books);
  await expect(
    navigation(page).getByRole("link", {
      name: "Retry audiobook page",
      exact: true,
    }),
  ).toHaveAttribute("href", href);
}

export async function recoverLibrary(
  page: Page,
  fixture: LibraryFixture,
  delivery: LibraryDelivery,
) {
  await openBooks(page, fixture, delivery);
  const first = fixture.books.slice(0, 60);
  const second = fixture.books.slice(60, 120);
  const lostHref = await nextHref(page);
  const lost = delivery.disrupt("lost", lostHref);
  await navigation(page)
    .getByRole("link", { name: "Next audiobooks", exact: true })
    .click();
  await failedPage(page, lostHref, first);
  expect(lost.observed()).toBe(true);
  expect(delivery.counts.lost).toBe(1);
  await pageLink(page, "Retry audiobook page", second, delivery);
  await pageLink(page, "First audiobooks", first, delivery);

  const heldHref = await nextHref(page);
  const held = delivery.disrupt("held", heldHref);
  const failures = delivery.counts.ownedFailures;
  await navigation(page)
    .getByRole("link", { name: "Next audiobooks", exact: true })
    .click();
  await expect.poll(() => delivery.counts.held).toBe(1);
  const started = Date.now();
  await failedPage(page, heldHref, first);
  expect(Date.now() - started).toBeGreaterThan(8_000);
  await expect.poll(() => delivery.counts.ownedFailures).toBe(failures + 1);
  held.release();
  await delivery.finish();
  await pageLink(page, "Retry audiobook page", second, delivery);
  await pageLink(page, "First audiobooks", first, delivery);

  const changedHref = await nextHref(page);
  const renamed = {
    ...fixture.books[59],
    name: `${fixture.books[59].name} renamed`,
  };
  fixture.remember(renamed.name);
  const operations = await fixture.owner.sync([
    { kind: "playlist_rename", playlist_id: renamed.id, name: renamed.name },
  ]);
  await rejectedAnchor(page, changedHref, first, delivery);
  let current = fixture.books
    .map((book) => (book.id === renamed.id ? renamed : book))
    .sort(order);
  await pageLink(page, "First audiobooks", current.slice(0, 60), delivery);
  const deleted = current[59].id;
  const deletedHref = await nextHref(page);
  operations.push(
    ...(await fixture.owner.sync([
      { kind: "playlist_delete", playlist_id: deleted },
    ])),
  );
  await rejectedAnchor(page, deletedHref, current.slice(0, 60), delivery);
  current = current.filter((book) => book.id !== deleted);
  await pageLink(page, "First audiobooks", current.slice(0, 60), delivery);

  const late = delivery.disrupt("held", await nextHref(page));
  const lateFailures = delivery.counts.ownedFailures;
  await navigation(page)
    .getByRole("link", { name: "Next audiobooks", exact: true })
    .click();
  await expect.poll(() => delivery.counts.held).toBe(2);
  delivery.sessions(fixture.owner.user.id);
  await signOutFromShell(page);
  await expect(
    page.getByRole("button", { name: "Sign in", exact: true }),
  ).toBeVisible();
  delivery.sessions(fixture.peer.user.id);
  await page
    .getByLabel("Username", { exact: true })
    .fill(fixture.peerCredentials.username);
  await page
    .getByLabel("Password", { exact: true })
    .fill(fixture.peerCredentials.password);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
  for (const cookie of await page.context().cookies())
    fixture.remember(cookie.value);
  await openLibrarySection(page, "Audiobooks");
  const peer = [
    { id: fixture.peerBook, name: fixture.peerName, kind: "local" as const },
  ];
  await assertBooks(page, peer);
  const token = await documentToken(page);
  await expect.poll(() => delivery.counts.ownedFailures).toBe(lateFailures + 1);
  expect(late.observed()).toBe(true);
  late.release();
  await delivery.finish();
  await assertBooks(page, peer);
  expect(await documentToken(page)).toBe(token);
  await networkQuiet(page);
  return { operations, renamed, deleted, books: current };
}

function order(left: Schema<"Playlist">, right: Schema<"Playlist">) {
  return left.name < right.name
    ? -1
    : left.name > right.name
      ? 1
      : left.id < right.id
        ? -1
        : left.id > right.id
          ? 1
          : 0;
}
async function rejectedAnchor(
  page: Page,
  href: string,
  books: Schema<"Playlist">[],
  delivery: LibraryDelivery,
) {
  delivery.expectAnchorConflict(href);
  const response = page.waitForResponse(
    (value) =>
      new URL(value.url()).pathname + new URL(value.url()).search === href &&
      value.request().headers()["x-inertia"] === "true",
  );
  await navigation(page)
    .getByRole("link", { name: "Next audiobooks", exact: true })
    .click();
  const rejected = await response;
  expect(rejected.status()).toBe(409);
  expect(rejected.headers()["cache-control"]).toBe("private, no-store");
  await failedPage(page, href, books);
}
