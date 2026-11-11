import { expect, test } from "@playwright/test";

const headers = (baseURL: string) => ({
  "x-hito-request": "1",
  origin: new URL(baseURL).origin,
});

test("Discover offers the library's genres, opens one and plays it, and keeps the service search below", async ({
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
      `/api/v1/libraries/${libraries.find((library: { personal?: boolean }) => !library.personal)!.id}/folders`,
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
        (
          await (
            await page.request.get("/api/v1/recordings?q=Hz&limit=1")
          ).json()
        ).total,
      { timeout: 30000 },
    )
    .toBeGreaterThanOrEqual(2);

  const navigation = page.getByRole("navigation", { name: "Main navigation" });
  await navigation.getByRole("link", { name: "Discover", exact: true }).click();
  await expect(page).toHaveURL(/\/discover$/);
  await expect(
    page.getByRole("heading", { level: 1, name: "Discover" }),
  ).toBeVisible();
  await expect(
    navigation.getByRole("link", { name: "Discover", exact: true }),
  ).toHaveAttribute("aria-current", "page");

  const genres = page.getByRole("region", { name: "Genres you own" });
  const tile = genres.getByRole("link", { name: /^Fixture tone/ });
  await expect.poll(() => tile.count(), { timeout: 30000 }).toBe(1);
  await expect(tile).toContainText("2 tracks");
  await expect(
    page.getByRole("region", { name: "Start a radio from an artist" }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("heading", { name: "Outside your library" }),
  ).toBeVisible();
  await page.screenshot({
    path: "/workspace/test-results/discover-desktop.png",
  });

  await tile.click();
  await expect(page).toHaveURL(/\/genres\?genre=fixture%20tone$/);
  await expect(
    page.getByRole("heading", { level: 1, name: "Fixture tone" }),
  ).toBeVisible();
  await expect(page.getByText("2 tracks in your library")).toBeVisible();
  await expect(
    navigation.getByRole("link", { name: "Discover", exact: true }),
  ).toHaveAttribute("aria-current", "page");
  await expect(page.getByRole("table").getByRole("row")).toHaveCount(3);
  await page.screenshot({ path: "/workspace/test-results/genre-desktop.png" });
  await page.getByRole("button", { name: "Play Fixture tone tracks" }).click();
  await expect(
    page.getByRole("contentinfo", { name: "Audio player" }),
  ).toContainText(/Fixture (440|660) Hz/);

  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/discover");
  await expect(
    genres.getByRole("link", { name: /^Fixture tone/ }),
  ).toBeVisible();
  for (const link of await navigation.getByRole("link").all()) {
    expect((await link.boundingBox())!.height).toBeGreaterThanOrEqual(44);
  }
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ),
  ).toBe(true);
  await page.screenshot({
    path: "/workspace/test-results/discover-mobile.png",
  });
  await page.goto("/genres?genre=fixture%20tone");
  await page.screenshot({ path: "/workspace/test-results/genre-mobile.png" });
  expect((await page.request.get("/genres?genre=nothing-here")).status()).toBe(
    404,
  );
  expect(errors).toEqual([]);
});
