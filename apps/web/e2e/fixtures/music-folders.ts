import { expect, type APIRequestContext, type Page } from "@playwright/test";

export async function addMusicFolders(page: Page, folders: string[]) {
  await page.goto("/settings?section=music");
  for (const folder of folders) {
    await page
      .getByRole("button", { name: "Add music folder", exact: true })
      .click();
    const picker = page.getByRole("dialog", {
      name: "Add a music folder",
      exact: true,
    });
    await picker
      .getByRole("button", { name: `Open ${folder}`, exact: true })
      .click();
    await picker
      .getByRole("button", { name: "Use this folder", exact: true })
      .click();
    await expect(picker).toBeHidden();
  }
  for (const folder of folders)
    await expect(
      page
        .getByRole("list", { name: "Connected music folders" })
        .getByRole("listitem")
        .filter({
          has: page.getByRole("heading", { name: folder, exact: true }),
        })
        .filter({ hasText: "1 file read" }),
    ).toHaveCount(1, { timeout: 20_000 });
}

export async function connectFixtureFolders(
  administrator: APIRequestContext,
  baseURL: string,
) {
  const headers = { "x-hito-request": "1", origin: new URL(baseURL).origin };
  const libraries: { id: string; personal?: boolean }[] = await (
    await administrator.get("/api/v1/libraries")
  ).json();
  const shared = libraries.find((library) => !library.personal)!;
  for (const [id, name] of [
    ["fixture-music", "Music"],
    ["fixture-archive", "Archive"],
  ]) {
    const connected = await administrator.post(
      `/api/v1/libraries/${shared.id}/folders`,
      {
        headers,
        data: { directory: { directory_id: id, components: [] }, name },
      },
    );
    expect([201, 409]).toContain(connected.status());
  }
  await expect
    .poll(
      async () =>
        (await (await administrator.get("/api/v1/releases?limit=1")).json())
          .total,
      { timeout: 30_000 },
    )
    .toBeGreaterThanOrEqual(2);
}
