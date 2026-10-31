import { expect, test } from "@playwright/test";

const QUEUE = "**/api/v1/artist-matching/review-queue?limit=50";
const NOVA = "44444444-4444-4444-8444-444444444444";
const OTHER = "55555555-5555-4555-8555-555555555555";

const candidate = (
  id: string,
  note: string,
  country: string,
  titles: number,
) => ({
  provider_artist_id: id,
  name: "Nova Quartet",
  kind: "group",
  disambiguation: note,
  country,
  corroborated_titles: titles,
  in_library: false,
});

const item = {
  artist_id: "credit-one",
  name: "Nova Quartet",
  undecided_tracks: 4,
  candidates: [
    candidate(NOVA, "Lisbon fado band", "PT", 1),
    candidate(OTHER, "Brazilian jazz trio", "BR", 0),
  ],
};

test("a library without undecided credits shows no confirmation section", async ({
  page,
}) => {
  const read = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname ===
        "/api/v1/artist-matching/review-queue" &&
      response.request().method() === "GET",
  );
  await page.goto("/artists");
  const queue = await (await read).json();
  expect(Object.keys(queue).sort()).toEqual([
    "items",
    "limit",
    "offset",
    "total",
  ]);
  expect(queue.total).toBe(0);
  await expect(page.getByText(/Needs your confirmation/)).toHaveCount(0);
});

test("one click on Confirm applies the candidate and the row leaves the collapsed section", async ({
  page,
}) => {
  const errors: string[] = [];
  const posted: { path: string; body: Record<string, unknown> }[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  let queue = [item];
  await page.route(QUEUE, (route) =>
    route.fulfill({
      json: { items: queue, total: queue.length, offset: 0, limit: 50 },
    }),
  );
  await page.route(
    "**/api/v1/artist-matching/review-queue/*/confirm",
    (route) => {
      posted.push({
        path: new URL(route.request().url()).pathname,
        body: route.request().postDataJSON(),
      });
      queue = [];
      return route.fulfill({
        json: {
          request_id: posted[0].body.request_id,
          artist_id: item.artist_id,
          provider_artist_id: NOVA,
          applied: 4,
          kept: 0,
          remaining: 0,
        },
      });
    },
  );

  await page.goto("/artists");
  const summary = page.getByText("Needs your confirmation (1)");
  await expect(summary).toBeVisible();
  const confirm = page.getByRole("button", {
    name: "Confirm Nova Quartet, Lisbon fado band, for Nova Quartet",
  });
  await expect(confirm, "collapsed by default").toBeHidden();

  await summary.focus();
  await page.keyboard.press("Enter");
  await expect(confirm).toBeVisible();
  await expect(page.getByText(/Brazilian jazz trio/)).toBeVisible();
  await confirm.click();

  await expect(page.getByText(/Needs your confirmation/)).toHaveCount(0);
  expect(posted).toHaveLength(1);
  expect(posted[0].path).toBe(
    "/api/v1/artist-matching/review-queue/credit-one/confirm",
  );
  expect(posted[0].body.provider_artist_id).toBe(NOVA);
  expect(errors).toEqual([]);
});

test("None of these removes the row without choosing an artist", async ({
  page,
}) => {
  let queue = [item];
  const dismissed: Record<string, unknown>[] = [];
  await page.route(QUEUE, (route) =>
    route.fulfill({
      json: { items: queue, total: queue.length, offset: 0, limit: 50 },
    }),
  );
  await page.route(
    "**/api/v1/artist-matching/review-queue/*/dismiss",
    (route) => {
      dismissed.push(route.request().postDataJSON());
      queue = [];
      return route.fulfill({
        json: { artist_id: item.artist_id, dismissed: 2 },
      });
    },
  );
  await page.goto("/artists");
  await page.getByText("Needs your confirmation (1)").click();
  await page
    .getByRole("button", { name: "None of these for Nova Quartet" })
    .click();
  await expect(page.getByText(/Needs your confirmation/)).toHaveCount(0);
  expect(dismissed).toEqual([{}]);
});
