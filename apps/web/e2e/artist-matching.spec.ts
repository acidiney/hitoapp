import { expect, test } from "@playwright/test";

test("the Artists page runs one silent tag-matching pass without any provider request and stays usable", async ({
  page,
}) => {
  const errors: string[] = [];
  const outbound: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (request) => {
    const url = new URL(request.url());
    if (url.pathname.startsWith("/api/v1/") && request.method() !== "GET")
      outbound.push(`${request.method()} ${url.pathname}`);
  });

  const pass = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname === "/api/v1/artist-matching" &&
      response.request().method() === "POST",
  );
  await page.goto("/artists");
  const response = await pass;
  expect(response.status()).toBe(200);
  const receipt = await response.json();
  expect(Object.keys(receipt).sort()).toEqual([
    "matched",
    "next_after",
    "request_id",
    "skipped_ambiguous",
    "skipped_declined",
  ]);
  expect(receipt.next_after).toBeNull();
  await expect(
    page.getByRole("heading", { name: "Artists", exact: true }),
  ).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);

  await page.reload();
  await expect(
    page.getByRole("heading", { name: "Artists", exact: true }),
  ).toBeVisible();
  await page.waitForTimeout(500);
  expect(
    outbound.filter((entry) => entry === "POST /api/v1/artist-matching"),
    "A pass that just finished is not repeated within ten minutes",
  ).toHaveLength(1);
  expect(outbound.every((entry) => entry.endsWith("/artist-matching"))).toBe(
    true,
  );
  expect(errors).toEqual([]);
});
