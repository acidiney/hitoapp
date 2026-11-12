import { expect, test, type Page } from "@playwright/test";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";
import { plainCandidate, timedCandidate } from "./fixtures/lyrics-provider";
import { libraryTrack, withProvider } from "./fixtures/lyrics-library";

async function playFromLibrary(page: Page, phone: boolean) {
  await page.goto("/library?q=Hz");
  if (!phone)
    await page.getByRole("button", { name: "List view", exact: true }).click();
  await page
    .getByRole("table", { name: "Tracks", exact: true })
    .locator("tbody tr")
    .filter({ hasText: "Fixture 440 Hz" })
    .getByRole("button", { name: "Play Fixture 440 Hz", exact: true })
    .click();
  await page
    .getByRole("button", {
      name: phone ? "Expand player" : "Open now playing",
      exact: true,
    })
    .click();
  const dialog = page.getByRole("dialog", { name: "Now playing" });
  if (phone)
    await dialog
      .getByRole("group", { name: "Now playing panels" })
      .getByRole("button", { name: "Lyrics", exact: true })
      .click();
  return dialog.getByRole("region", { name: "Lyrics", exact: true });
}

const viewports = [
  ["desktop", { width: 1440, height: 900 }],
  ["phone", { width: 390, height: 844 }],
] as const;

for (const [name, viewport] of viewports)
  test(`Now playing on ${name} follows timed lyrics the real backend chose over an earlier plain result`, async ({
    page,
    baseURL,
  }) => {
    if (!sharedAdminSession) await enterAdmin(page);
    await page.setViewportSize(viewport);
    const track = await libraryTrack(page, "Fixture 440 Hz");
    await withProvider(
      page,
      async (provider) => {
        provider.state.candidates = [
          plainCandidate("plain-a", track.facts),
          timedCandidate("timed-b", track.facts),
        ];
        const lyrics = await playFromLibrary(page, name === "phone");
        await expect(lyrics.locator('[aria-current="true"]')).toHaveText(
          "Timed river line",
          { timeout: 15_000 },
        );
        await expect(lyrics.getByText("Plain river line")).toHaveCount(0);
        await expect(lyrics.getByText(/^Synced/)).toBeVisible();
        await expect(lyrics.getByText("None found")).toHaveCount(0);
        await expect(
          page.getByRole("button", { name: "Pause" }).first(),
        ).toBeVisible();
        const playing = await page.evaluate(
          () => document.querySelector("audio")?.paused === false,
        );
        expect(playing).toBe(true);
        expect(provider.reads()).toEqual(["lyrics:plain-a", "lyrics:timed-b"]);
        await page.getByRole("button", { name: "Pause" }).first().click();
      },
      baseURL!,
    );
  });
