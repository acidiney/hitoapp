import { expect, test, type Page } from "@playwright/test";
import { albumEdition } from "./fixtures/album-edition-provider";
import { longTrack, savedAlbum } from "./fixtures/saved-album";
import { createMembers, memberPassword } from "./fixtures/member-accounts";

const stats = (page: Page) => page.locator(".album-header .album-stats");

test("a saved album edition with one unknown track length shows its track count without a length", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const album = await savedAlbum(page, baseURL!, [
    longTrack,
    null,
    longTrack,
    longTrack,
  ]);
  try {
    await expect(stats(page)).toHaveText(/ · 4 tracks$/);
    await album.region
      .getByRole("listitem")
      .nth(0)
      .getByRole("button", { name: /^Play / })
      .click();
    const player = page.locator("audio[data-hito-player]");
    await expect
      .poll(() =>
        player.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(0.2);
    await expect(stats(page)).toHaveText(/ · 4 tracks$/);
    expect(album.fixture.state.unexpected).toBe(0);
  } finally {
    await album.close();
  }
});

test("a saved album edition with known track lengths shows the total the server reports", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const album = await savedAlbum(page, baseURL!, [
    longTrack,
    longTrack,
    longTrack,
    longTrack,
  ]);
  try {
    await expect(stats(page)).toHaveText(/ · 4 tracks · 1 hr 0 min$/);
    expect(album.fixture.state.unexpected).toBe(0);
  } finally {
    await album.close();
  }
});

test("another account does not see someone else's chosen edition or its totals", async ({
  page,
  browser,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const album = await savedAlbum(page, baseURL!, [
    longTrack,
    longTrack,
    longTrack,
    longTrack,
  ]);
  let members: Awaited<ReturnType<typeof createMembers>> | undefined;
  let peer: Awaited<ReturnType<typeof browser.newContext>> | undefined;
  try {
    await expect(stats(page)).toHaveText(/ · 4 tracks · 1 hr 0 min$/);
    members = await createMembers(browser, baseURL!, "saved-stats", 1);
    peer = await browser.newContext({ baseURL });
    const login = await peer.request.post("/api/v1/login", {
      headers: { "x-hito-request": "1", origin: new URL(baseURL!).origin },
      data: { username: members.names[0], password: memberPassword },
    });
    expect(login.status()).toBe(200);
    const other = await peer.newPage();
    const dispatched = album.fixture.calls.length;
    const response = await other.goto(`/albums/${album.release}`);
    expect(response?.status()).toBe(404);
    expect(album.fixture.calls.length).toBe(dispatched);
    await expect(
      other.getByText(album.fixture.titles[albumEdition.first]),
    ).toHaveCount(0);
    await expect(other.getByText(/1 hr 0 min/)).toHaveCount(0);
    await expect(
      other.getByRole("region", { name: "Album edition track list" }),
    ).toHaveCount(0);
  } finally {
    try {
      await peer?.close();
    } finally {
      try {
        await members?.close();
      } finally {
        await album.close();
      }
    }
  }
});
