import { expect, test } from "@playwright/test";

test("the Artists page shows the server's matching status, starts no pass of its own and stays usable", async ({
  page,
}) => {
  const errors: string[] = [];
  const writes: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (request) => {
    const url = new URL(request.url());
    if (url.pathname.startsWith("/api/v1/") && request.method() !== "GET")
      writes.push(`${request.method()} ${url.pathname}`);
  });

  const read = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname === "/api/v1/artist-matching/status" &&
      response.request().method() === "GET",
  );
  await page.goto("/artists");
  const response = await read;
  expect(response.status()).toBe(200);
  const status = await response.json();
  expect(Object.keys(status).sort()).toEqual([
    "enabled",
    "last_finished_at",
    "last_matched",
    "matched_so_far",
    "state",
  ]);
  await expect(
    page.getByRole("heading", { name: "Artists", exact: true }),
  ).toBeVisible();
  if (status.enabled)
    await expect(page.locator(".artist-matching-status")).toContainText(
      /Matching artists…|matched|No run yet/,
    );
  await expect(page.getByRole("alert")).toHaveCount(0);

  await page.reload();
  await expect(
    page.getByRole("heading", { name: "Artists", exact: true }),
  ).toBeVisible();
  await page.waitForTimeout(500);
  expect(writes, "Opening the page never starts a matching pass").toEqual([]);
  expect(errors).toEqual([]);
});
