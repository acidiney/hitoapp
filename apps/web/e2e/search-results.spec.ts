import { expect, test, type Page } from "@playwright/test";

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
    pill: getComputedStyle(input.closest("form")!).borderTopWidth,
  }));
  expect(rings.inner).toBe("none");
  expect(rings.pill).toBe("2px");
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

const connection = {
  checked_at: 0,
  credential_set: true,
  id: "conn1",
  revision: 1,
  service_id: "svc1",
  manifest: { name: "YouTube", capabilities: ["search", "sources"] },
};

async function openSearchWithService(
  page: Page,
  baseURL: string,
  unsourced: boolean,
) {
  await page.goto("/home");
  const libraries = await (await page.request.get("/api/v1/libraries")).json();
  const connected = await page.request.post(
    `/api/v1/libraries/${libraries[0].id}/folders`,
    {
      headers: headers(baseURL),
      data: {
        directory: { directory_id: "fixture-music", components: [] },
        name: "Music",
      },
    },
  );
  expect([201, 409]).toContain(connected.status());
  await expect
    .poll(
      async () =>
        (await (await page.request.get("/api/v1/recordings?limit=1")).json())
          .total,
      { timeout: 30000 },
    )
    .toBeGreaterThanOrEqual(2);
  await page.route("**/search**", async (route) => {
    if (route.request().headers()["x-inertia"] !== "true")
      return route.continue();
    const response = await route.fetch();
    const body = await response.json();
    body.props.connections = [connection];
    body.props.service_names = { svc1: "YouTube" };
    if (unsourced)
      for (const recording of body.props.recordings?.items ?? []) {
        recording.available = false;
        recording.plays_from_services = false;
      }
    await route.fulfill({ response, json: body });
  });
  await page
    .getByRole("navigation", { name: "Main navigation" })
    .getByRole("link", { name: /^Search/ })
    .click();
  await page
    .getByRole("searchbox", { name: "Search", exact: true })
    .fill("Fixture");
}

for (const width of [1440, 1100, 800]) {
  test(`Find stays fully visible and clickable on a hovered unsourced row at ${width}px`, async ({
    page,
    baseURL,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await openSearchWithService(page, baseURL!, true);
    const row = page.locator(".result-row:has(.result-row-action)").first();
    await expect(row).toBeVisible();
    for (const reveal of ["hover", "focus"] as const) {
      if (reveal === "hover") await row.hover();
      else await row.getByRole("link", { name: "Find" }).focus();
      const find = row.getByRole("link", { name: "Find" });
      await expect(find).toBeVisible();
      const clipped = await find.evaluate((link) => {
        const box = link.getBoundingClientRect();
        const rowBox = link.closest(".result-row")!.getBoundingClientRect();
        const stateBox = link.parentElement!.getBoundingClientRect();
        const hit = document.elementFromPoint(
          box.x + box.width / 2,
          box.y + box.height / 2,
        );
        const others = [
          ...link
            .closest(".result-row")!
            .querySelectorAll(".action-menu, .track-actions .icon-button"),
        ].some((other) => {
          const rect = other.getBoundingClientRect();
          return rect.right > box.left && rect.left < box.right;
        });
        return {
          covered: hit !== link,
          overlapped: others,
          outside:
            box.right > stateBox.right + 0.5 || box.right > rowBox.right + 0.5,
        };
      });
      expect(clipped).toEqual({
        covered: false,
        overlapped: false,
        outside: false,
      });
    }
    await row.hover();
    await row.getByRole("link", { name: "Find" }).click();
    await expect(page).not.toHaveURL(/\/search/);
  });
}

test("a service result can be queued from its row menu", async ({
  page,
  baseURL,
}) => {
  await openSearchWithService(page, baseURL!, false);
  const recordings = (
    await (await page.request.get("/api/v1/recordings?limit=1")).json()
  ).items as { id: string; title: string }[];
  await page.route("**/api/v1/plugins/conn1/search", (route) =>
    route.fulfill({
      json: {
        items: [
          {
            id: "y1",
            title: "Night drive",
            artist: "Someone",
            duration_ms: 1000,
          },
        ],
        next_cursor: null,
      },
    }),
  );
  await page.route("**/api/v1/plugins/conn1/recordings", (route) =>
    route.fulfill({
      json: { ...recordings[0], available: true, plays_from_services: true },
    }),
  );
  await page.getByRole("button", { name: "YouTube" }).click();
  await expect(page.getByText("Night drive")).toBeVisible();
  await page.getByRole("button", { name: "Actions for Night drive" }).click();
  await page.getByRole("menuitem", { name: "Add to queue" }).click();
  await page.getByRole("button", { name: "Open queue", exact: true }).click();
  await expect(page.locator("#queue-panel[data-open]")).toBeVisible();
  await expect(page.getByText(recordings[0].title).first()).toBeVisible();
});
