import { expect, test } from "@playwright/test";
import { chooseOption } from "./fixtures/listbox";
import { openLibrarySection } from "./fixtures/library-sections";

const headers = (baseURL: string) => ({
  "x-hito-request": "1",
  origin: new URL(baseURL).origin,
});

test("Albums and Library tracks sort from a listbox, keep the choice in the address and across a reload", async ({
  page,
  baseURL,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/home");
  const libraries = await (await page.request.get("/api/v1/libraries")).json();
  for (const [id, name] of [
    ["fixture-music", "Music"],
    ["fixture-archive", "Archive"],
  ]) {
    const connected = await page.request.post(
      `/api/v1/libraries/${libraries[0].id}/folders`,
      {
        headers: headers(baseURL!),
        data: { directory: { directory_id: id, components: [] }, name },
      },
    );
    expect([201, 409]).toContain(connected.status());
  }
  await expect
    .poll(
      async () =>
        (await (await page.request.get("/api/v1/releases?limit=1")).json())
          .total,
      { timeout: 30000 },
    )
    .toBeGreaterThanOrEqual(2);

  await openLibrarySection(page, "Albums");
  await expect(page).toHaveURL(/\/albums$/);
  const sort = page.getByRole("combobox", { name: "Sort by" });
  await expect(sort).toHaveText("Recently added");
  const cards = page.locator(".album-card h2");

  await chooseOption(sort, "Title A–Z");
  await expect(page).toHaveURL(/\/albums\?sort=title$/);
  await expect(sort).toHaveText("Title A–Z");
  await expect(cards.first()).toHaveText("Fixture collection 440 Hz");
  await expect(cards.last()).toHaveText("Fixture collection 660 Hz");

  await chooseOption(sort, "Year, newest first");
  await expect(page).toHaveURL(/\/albums\?sort=year$/);
  await page.reload();
  await expect(sort).toHaveText("Year, newest first");
  await expect(cards).toHaveCount(2);

  await chooseOption(sort, "Recently added");
  await expect(page).toHaveURL(/\/albums$/);
  await expect(sort).toHaveText("Recently added");

  await page
    .getByRole("navigation", { name: "Main navigation", exact: true })
    .getByRole("link", { name: "Library", exact: true })
    .click();
  await expect(page).toHaveURL(/\/library/);
  const trackSort = page.getByRole("combobox", { name: "Sort by" });
  await chooseOption(trackSort, "Artist A–Z");
  await expect(page).toHaveURL(/\/library\?sort=artist$/);
  await expect(trackSort).toHaveText("Artist A–Z");
  await expect(page.getByText("Fixture 440 Hz").first()).toBeVisible();
  expect(errors).toEqual([]);
});
