import { expect, test } from "@playwright/test";

const headers = (baseURL: string) => ({
  "x-hito-request": "1",
  origin: new URL(baseURL).origin,
});

test("Search lists each match once with a count per section and a helpful empty state", async ({
  page,
  baseURL,
}) => {
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
        (await (await page.request.get("/api/v1/recordings?limit=1")).json())
          .total,
      { timeout: 30000 },
    )
    .toBeGreaterThanOrEqual(2);

  await page.goto("/search?q=Fixture");
  await expect(page.getByRole("heading", { name: "Top result" })).toHaveCount(
    0,
  );
  const tracks = page.getByRole("region", { name: "Tracks" });
  await expect(tracks).toContainText(/\d+ results?/);
  await expect(
    tracks.getByRole("link", { name: "Fixture 440 Hz", exact: true }),
  ).toHaveCount(1);
  await expect(page.getByText("Fixture 440 Hz", { exact: true })).toHaveCount(
    1,
  );
  await expect(page.getByRole("region", { name: "Albums" })).toContainText(
    /\d+ results?/,
  );
  await page.screenshot({
    path: "/workspace/test-results/search-results-desktop.png",
  });

  await page.goto("/search?q=zzqqxx");
  await expect(
    page.getByRole("heading", {
      name: "Nothing in your library matches “zzqqxx”",
    }),
  ).toBeVisible();
  await expect(page.getByText(/Check the spelling/)).toBeVisible();
  await expect(
    page.getByRole("link", { name: "Connect a music service" }),
  ).toHaveAttribute("href", "/settings?section=plugins");
  await page.screenshot({
    path: "/workspace/test-results/search-none-desktop.png",
  });
});
