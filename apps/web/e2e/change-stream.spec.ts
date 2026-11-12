import { randomUUID } from "node:crypto";
import { expect, test } from "@playwright/test";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";
import { createPlaylist } from "./fixtures/playlist-dialogs";
import { enterEditMode } from "./fixtures/playlist-edit";

test("a playlist created and renamed on one page appears on another page of the same account without a reload", async ({
  page: writer,
}) => {
  if (!sharedAdminSession) await enterAdmin(writer);
  const reader = await writer.context().newPage();
  const suffix = randomUUID().slice(0, 8);
  const name = `Stream ${suffix}`;
  const renamed = `Stream renamed ${suffix}`;

  await reader.goto("/library");
  await expect(
    reader.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
  const streams = reader.waitForRequest((request) =>
    request.url().endsWith("/api/v1/changes/events"),
  );
  await reader.reload();
  await streams;
  await reader.evaluate(() => {
    (window as unknown as { loadMarker: string }).loadMarker = "same-load";
  });
  const sidebar = reader.locator(".sidebar-playlists");

  await writer.goto("/playlists");
  await createPlaylist(writer, name);
  await expect(sidebar.getByRole("link", { name })).toBeVisible();

  await writer
    .locator(".sidebar-playlists")
    .getByRole("link", { name })
    .click();
  await enterEditMode(writer);
  await writer.getByRole("textbox", { name: "Name" }).fill(renamed);
  await writer
    .getByRole("button", { name: "Done editing", exact: true })
    .click();
  await expect(
    writer.getByRole("heading", { name: renamed, exact: true }),
  ).toBeVisible();

  await expect(sidebar.getByRole("link", { name: renamed })).toBeVisible();
  await expect(sidebar.getByRole("link", { name, exact: true })).toHaveCount(0);
  expect(
    await reader.evaluate(
      () => (window as unknown as { loadMarker?: string }).loadMarker,
    ),
  ).toBe("same-load");
});
