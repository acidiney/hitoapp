import { expect, test } from "@playwright/test";
import { connectCatalogArtist } from "./fixtures/catalog-album-setup";
import { createMembers, memberPassword } from "./fixtures/member-accounts";

test("opening a credit that is fully identified lands on the chosen artist page", async ({
  browser,
  page,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const accounts = await createMembers(browser, baseURL!, "unified-artist", 1);
  const member = await browser.newContext({
    baseURL,
    storageState: { cookies: [], origins: [] },
  });
  try {
    const login = await member.request.post("/api/v1/login", {
      headers: { "x-hito-request": "1", origin: new URL(baseURL!).origin },
      data: { username: accounts.names[0], password: memberPassword },
    });
    expect(login.status()).toBe(200);
    const memberPage = await member.newPage();
    const artist = await connectCatalogArtist(
      memberPage,
      baseURL!,
      page.request,
    );
    try {
      expect(artist.anchor.artist_id).toBeTruthy();
      await memberPage.goto(`/artists/${artist.anchor.artist_id}`);
      await expect(memberPage).toHaveURL(/\/artist-groups\/[^/]+$/);
      await expect(memberPage.getByRole("heading", { level: 1 })).toBeVisible();
    } finally {
      await artist.close();
    }
  } finally {
    await member.close();
    await accounts.close();
  }
});

test("the album artist in the header is a link to that artist", async ({
  page,
}) => {
  const releases = (
    await (await page.request.get("/api/v1/releases?limit=50")).json()
  ).items as { id: string; artist_id: string | null; artist: string | null }[];
  const release = releases.find((item) => item.artist_id && item.artist);
  test.skip(!release, "No fixture album has an artist credit");
  await page.goto(`/albums/${release!.id}`);
  const link = page
    .locator(".album-header")
    .getByRole("link", { name: release!.artist! });
  await expect(link).toHaveAttribute(
    "href",
    new RegExp(`^/(artists|artist-groups)/`),
  );
  await link.focus();
  await expect(link).toBeFocused();
  await link.click();
  await expect(page).toHaveURL(/\/artist(s|-groups)\//);
});
