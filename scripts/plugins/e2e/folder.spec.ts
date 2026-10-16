import {
  expect,
  test,
} from "/workspace/apps/web/node_modules/@playwright/test";

test("a person installs the real folder demo, discovers, adopts, plays, seeks, and removes it", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/settings");
  await expect(
    page.getByRole("heading", { name: "Set up Hito", exact: true }),
  ).toBeVisible();
  await page.getByLabel("Username", { exact: true }).fill("plugin-demo-admin");
  await page
    .getByLabel("Password", { exact: true })
    .fill("isolated folder demo browser password");
  await page
    .getByRole("button", { name: "Create administrator", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
  await page.goto("/settings");
  await page.getByRole("tab", { name: "Plugins", exact: true }).click();
  await page.getByText("Services for this server", { exact: true }).click();
  await page
    .getByLabel("Service address", { exact: true })
    .fill("http://plugin-folder:8090/");
  const offeredResponse = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname === "/api/v1/plugins/services" &&
      response.request().method() === "POST",
  );
  await page
    .getByRole("button", { name: "Offer service", exact: true })
    .click();
  const offered = await offeredResponse;
  expect(offered.status()).toBe(201);
  expect(offered.request().postDataJSON()).toEqual({
    base_url: "http://plugin-folder:8090/",
  });
  expect(await offered.json()).toMatchObject({
    name: "Hito music folder",
    base_url: "http://plugin-folder:8090/",
  });
  await expect(
    page.getByRole("combobox", { name: "Service", exact: true }),
  ).toHaveValue(/.+/);
  await expect(
    page.getByRole("combobox", { name: "Service", exact: true }),
  ).toContainText("Hito music folder");
  await page.getByText("Services for this server", { exact: true }).click();
  await page
    .getByRole("button", { name: "Connect service", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Hito music folder", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Check connection", exact: true })
    .click();
  await expect(
    page.getByText("Connection checked.", { exact: true }),
  ).toBeVisible();
  await page.screenshot({
    path: "/workspace/test-results/plugins-folder/settings-connected.png",
  });
  await page.goto("/discover");
  await expect(
    page.getByRole("combobox", { name: "Music service", exact: true }),
  ).toHaveValue(/.+/);
  await page
    .getByRole("searchbox", { name: "Search music", exact: true })
    .fill("Hito demo");
  await page.getByRole("button", { name: "Search", exact: true }).click();
  await expect(page.locator(".provider-result")).toHaveCount(3);
  expect(
    (await (await page.request.get("/api/v1/recordings?q=Hito%20demo")).json())
      .total,
  ).toBe(0);
  await page
    .getByRole("button", {
      name: "Add Hito demo tone 440 Hz to library",
      exact: true,
    })
    .click();
  await expect(
    page.getByRole("link", {
      name: "View Hito demo tone 440 Hz in library",
      exact: true,
    }),
  ).toBeVisible();
  expect(
    (await (await page.request.get("/api/v1/recordings?q=Hito%20demo")).json())
      .total,
  ).toBe(1);
  const audio = page.locator("audio");
  expect(
    await audio.evaluate((element: HTMLAudioElement) =>
      element.getAttribute("src"),
    ),
  ).toBeNull();
  await page
    .getByRole("button", { name: "Play Hito demo tone 554 Hz", exact: true })
    .click();
  await expect
    .poll(() =>
      audio.evaluate((element: HTMLAudioElement) => element.currentTime),
    )
    .toBeGreaterThan(0.2);
  const source = await audio.evaluate(
    (element: HTMLAudioElement) => element.src,
  );
  expect(new URL(source).pathname).toMatch(
    /^\/api\/v1\/sources\/[a-f0-9-]+\/audio$/,
  );
  const seek = page.getByRole("slider", { name: "Seek", exact: true });
  await seek.focus();
  await seek.press("PageUp");
  await expect
    .poll(() =>
      audio.evaluate((element: HTMLAudioElement) => element.currentTime),
    )
    .toBeGreaterThan(2);
  const firstByte = await page.request.get(source, {
    headers: { range: "bytes=0-15" },
  });
  expect(firstByte.status()).toBe(206);
  expect((await firstByte.body()).length).toBe(16);
  expect(firstByte.headers()["content-range"]).toMatch(/^bytes 0-15\/[0-9]+$/);
  const body = await (
    await page.request.get("/api/v1/recordings?q=Hito%20demo")
  ).json();
  expect(body.total).toBe(2);
  expect(body.items[0].release_id).toBeTruthy();
  expect(body.items[0].release_id).toBe(body.items[1].release_id);
  const recordingIds = body.items.map((item: { id: string }) => item.id).sort();
  const before = await audio.evaluate(
    (element: HTMLAudioElement) => element.currentTime,
  );
  await page.screenshot({
    path: "/workspace/test-results/plugins-folder/discover-playing.png",
  });
  await page.setViewportSize({ width: 390, height: 844 });
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
  ).toBe(false);
  await page.screenshot({
    path: "/workspace/test-results/plugins-folder/discover-mobile.png",
  });
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page
    .getByRole("navigation", { name: "Main navigation" })
    .getByRole("link", { name: "Library", exact: true })
    .click();
  await expect(
    page.getByRole("button", {
      name: "Play Hito demo tone 440 Hz",
      exact: true,
    }),
  ).toBeEnabled();
  await expect
    .poll(() =>
      audio.evaluate((element: HTMLAudioElement) => element.currentTime),
    )
    .toBeGreaterThan(before);
  await page.goto("/settings");
  await page.getByRole("tab", { name: "Plugins", exact: true }).click();
  await page.getByRole("button", { name: "Remove", exact: true }).click();
  await page
    .getByRole("button", { name: "Remove plugin", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Hito music folder", exact: true }),
  ).toHaveCount(0);
  const retained = await (
    await page.request.get("/api/v1/recordings?q=Hito%20demo")
  ).json();
  expect(retained.total).toBe(2);
  expect(retained.items.map((item: { id: string }) => item.id).sort()).toEqual(
    recordingIds,
  );
  expect((await page.request.get(source)).status()).toBe(404);
  await page.goto("/library?q=Hito%20demo");
  for (const frequency of [440, 554])
    await expect(
      page.getByRole("button", {
        name: `Play Hito demo tone ${frequency} Hz`,
        exact: true,
      }),
    ).toBeDisabled();
  await page.screenshot({
    path: "/workspace/test-results/plugins-folder/library-after-removal.png",
  });
  expect(errors).toEqual([]);
});
