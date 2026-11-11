import { expect, test } from "@playwright/test";

const headers = (baseURL: string) => ({
  "x-hito-request": "1",
  origin: new URL(baseURL).origin,
});

test("Search leads with a top result, lists tracks with their availability and has a helpful empty state", async ({
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

  await page.goto("/search?q=Fixture%20440%20Hz");
  const field = page.getByRole("searchbox", { name: "Search", exact: true });
  await field.focus();
  const rings = await field.evaluate((input) => ({
    inner: getComputedStyle(input).outlineStyle,
    pill: getComputedStyle(input.closest("form")!).borderTopColor,
  }));
  expect(rings.inner).toBe("none");
  expect(rings.pill).toBe("rgb(111, 134, 255)");
  const top = page.getByRole("region", { name: "Top result" });
  await expect(
    top.getByRole("link", { name: "Fixture 440 Hz", exact: true }),
  ).toHaveCount(1);
  const tracks = page.getByRole("region", { name: "Tracks" });
  await expect(
    tracks.getByRole("link", { name: "Fixture 440 Hz", exact: true }),
  ).toHaveCount(1);
  await expect(tracks.getByText("Ready · Local").first()).toBeVisible();
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
    page
      .getByRole("link", { name: "Connect a music service" })
      .or(page.getByRole("button", { name: /^Search / })),
  ).toBeVisible();
  await page.screenshot({
    path: "/workspace/test-results/search-none-desktop.png",
  });
});
