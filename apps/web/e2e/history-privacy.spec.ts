import { expect, test, type Page } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import { openLibrarySection } from "./fixtures/library-sections";

test.use({ storageState: { cookies: [], origins: [] } });

const password = "history privacy listener password";

function headers(baseURL: string) {
  return { "x-hito-request": "1", origin: new URL(baseURL).origin };
}

async function prepareListener(page: Page, baseURL: string) {
  await page.goto("/");
  await page.getByLabel("Username", { exact: true }).fill("admin");
  await page
    .getByLabel("Password", { exact: true })
    .fill("docker browser test password");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
  const libraries: Schema<"Library">[] = await (
    await page.request.get("/api/v1/libraries")
  ).json();
  for (const [id, name] of [
    ["fixture-music", "Music"],
    ["fixture-archive", "Archive"],
  ]) {
    const connected = await page.request.post(
      `/api/v1/libraries/${libraries[0].id}/folders`,
      {
        headers: headers(baseURL),
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
  const created = await page.request.post("/api/v1/users", {
    headers: headers(baseURL),
    data: { username: "history-listener", password },
  });
  expect(created.status()).toBe(201);
  const user: Schema<"User"> = await created.json();
  const granted = await page.request.put(
    `/api/v1/libraries/${libraries[0].id}/members/${user.id}`,
    { headers: headers(baseURL) },
  );
  expect(granted.ok()).toBe(true);
  await page.request.post("/api/v1/logout", { headers: headers(baseURL) });
  await page.goto("/");
  await page.getByLabel("Username", { exact: true }).fill("history-listener");
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
}

async function listen(page: Page, title: string) {
  await page.goto("/library");
  const play = page.getByRole("button", {
    name: `Play ${title}`,
    exact: true,
  });
  await expect(play).toBeEnabled({ timeout: 20000 });
  await play.click();
  await expect
    .poll(() =>
      page
        .locator("audio[data-hito-player]")
        .evaluate((audio: HTMLAudioElement) => audio.currentTime),
    )
    .toBeGreaterThan(0.2);
  const reported = page.waitForResponse(
    (response) =>
      response.url().endsWith("/api/v1/sync") &&
      response.request().method() === "POST" &&
      (response.request().postData() ?? "").includes("progress_set"),
  );
  await page.getByRole("button", { name: "Pause", exact: true }).click();
  expect((await reported).status()).toBe(200);
}

async function historyTitles(page: Page): Promise<string[]> {
  const activity: Schema<"Activity"> = await (
    await page.request.get("/api/v1/activity")
  ).json();
  return activity.history.map((entry) => entry.title).sort();
}

test("pausing listening history stops new entries and deleting it empties History and Home", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(150_000);
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await prepareListener(page, baseURL!);

  await listen(page, "Fixture 440 Hz");
  await expect.poll(() => historyTitles(page)).toEqual(["Fixture 440 Hz"]);
  await openLibrarySection(page, "History");
  await expect(
    page.getByRole("button", { name: "Play Fixture 440 Hz", exact: true }),
  ).toBeVisible();

  await page.goto("/home");
  await expect(
    page.getByRole("heading", { name: "Recently played" }),
  ).toBeVisible();

  await page.goto("/settings?section=privacy");
  const card = page.getByRole("region", { name: "Listening history" });
  const saveHistory = card.getByRole("switch", {
    name: "Save listening history",
  });
  await expect(saveHistory).toHaveAttribute("aria-checked", "true");
  await saveHistory.click();
  await expect(saveHistory).toHaveAttribute("aria-checked", "false");
  await expect(card.getByText("Listening history is paused.")).toBeVisible();
  await page.reload();
  await expect(
    page
      .getByRole("region", { name: "Listening history" })
      .getByRole("switch", { name: "Save listening history" }),
  ).toHaveAttribute("aria-checked", "false");

  await listen(page, "Fixture 660 Hz");
  expect(await historyTitles(page)).toEqual(["Fixture 440 Hz"]);
  await page.goto("/history");
  await expect(
    page.getByRole("button", { name: "Play Fixture 440 Hz", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Play Fixture 660 Hz", exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByText("Listening history is paused, so new plays are not added."),
  ).toBeVisible();

  await page.goto("/settings?section=privacy");
  await page
    .getByRole("button", { name: "Delete listening history", exact: true })
    .click();
  const dialog = page.getByRole("dialog", {
    name: "Delete listening history?",
  });
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toBeHidden();
  expect(await historyTitles(page)).toEqual(["Fixture 440 Hz"]);
  await page
    .getByRole("button", { name: "Delete listening history", exact: true })
    .click();
  await page
    .getByRole("dialog", { name: "Delete listening history?" })
    .getByRole("button", { name: "Delete history" })
    .click();
  await expect(page.getByText("1 entry removed.")).toBeVisible();
  expect(await historyTitles(page)).toEqual([]);

  await page.goto("/history");
  await expect(
    page.getByRole("heading", { name: "Listening history is paused" }),
  ).toBeVisible();
  await page.goto("/home");
  const recentlyPlayed = page.getByRole("region", { name: "Recently played" });
  await expect(
    recentlyPlayed.getByRole("link", { name: /Fixture/ }),
  ).toHaveCount(0);
  await expect(recentlyPlayed.getByRole("list")).toHaveCount(0);

  await page.goto("/settings?section=privacy");
  const resumed = page
    .getByRole("region", { name: "Listening history" })
    .getByRole("switch", { name: "Save listening history" });
  await resumed.click();
  await expect(resumed).toHaveAttribute("aria-checked", "true");
  await listen(page, "Fixture 660 Hz");
  await expect.poll(() => historyTitles(page)).toEqual(["Fixture 660 Hz"]);
  expect(errors).toEqual([]);
});
