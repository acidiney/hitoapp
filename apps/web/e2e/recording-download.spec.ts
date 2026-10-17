import { expect, test, type Page } from "@playwright/test";
import type { components } from "../src/generated";
import { prepareDownloadLibrary, read } from "./fixtures/downloads";
import { providerFixture } from "./fixtures/provider";

type Schema = components["schemas"];
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

test.use({ trace: "off", screenshot: "off", video: "off" });

test.describe("explicit persistent recording download", () => {
  test.skip(
    process.env.HITO_RECORDING_DOWNLOAD_E2E !== "1",
    "Requires an opted-in fresh disposable download server",
  );

  test("a saved server copy plays without its provider and an old removal retry preserves a new copy", async ({
    page,
    context,
    baseURL,
  }) => {
    if (!baseURL)
      throw new Error("The disposable server URL must be configured");
    const fixture = await providerFixture("Download");
    const origin = new URL(baseURL).origin;
    const creations: string[] = [];
    const removals: string[] = [];
    let recordingPath = "";
    let pageErrors = 0;
    let browserAudioRequests = 0;
    let second: Page | null = null;
    page.on("pageerror", () => pageErrors++);
    context.on("request", (request) => {
      const url = new URL(request.url());
      if (url.origin !== origin) return;
      if (/^\/api\/v1\/sources\/[0-9a-f-]+\/audio$/.test(url.pathname))
        browserAudioRequests++;
      if (request.method() === "POST" && url.pathname === recordingPath) {
        const body: Schema["DownloadRequest"] = request.postDataJSON();
        creations.push(body.request_id);
      }
      if (
        request.method() === "DELETE" &&
        /^\/api\/v1\/downloads\/[0-9a-f-]+$/.test(url.pathname)
      )
        removals.push(url.pathname);
    });
    try {
      const library = await prepareDownloadLibrary(page, baseURL, fixture);
      recordingPath = `/api/v1/recordings/${library.recording}/download`;
      const details = `/recordings/${library.recording}`;
      await page.goto(details);
      const section = page.getByRole("region", {
        name: "Download",
        exact: true,
      });
      const download = section.getByRole("button", {
        name: "Download",
        exact: true,
      });
      await expect(download).toBeEnabled();
      const absent = await page.request.get(recordingPath);
      expect(absent.status()).toBe(404);
      await page
        .getByRole("button", { name: "Check availability", exact: true })
        .click();
      await expect(
        page.getByRole("button", { name: "Play track", exact: true }),
      ).toBeEnabled();
      expect(creations.length).toBe(0);
      expect(removals.length).toBe(0);
      expect(fixture.requests.length).toBe(0);
      expect(browserAudioRequests).toBe(0);
      const audio = page.locator("audio");
      expect(await audio.getAttribute("src")).toBeNull();
      expect(
        await audio.evaluate((element: HTMLAudioElement) => element.paused),
      ).toBe(true);
      const draft = page.getByLabel("Track title", { exact: true });
      await draft.fill("Unsent download title draft");
      const firstResponse = page.waitForResponse(
        (response) =>
          new URL(response.url()).pathname === recordingPath &&
          response.request().method() === "POST",
      );
      await download.click();
      const accepted = await firstResponse;
      expect(accepted.status()).toBe(202);
      const firstReceipt: Schema["DownloadReceipt"] = await accepted.json();
      expect(
        uuid.test(firstReceipt.intent_id) &&
          firstReceipt.recording_id === library.recording,
      ).toBe(true);
      await expect(section.getByRole("status")).toHaveText(
        "Saved on this server",
        { timeout: 30_000 },
      );
      await expect(draft).toHaveValue("Unsent download title draft");
      expect(creations.length).toBe(1);
      expect(
        uuid.test(creations[0]) && creations[0] === firstReceipt.request_id,
      ).toBe(true);
      expect(browserAudioRequests).toBe(0);
      expect(await audio.getAttribute("src")).toBeNull();
      expect(
        await audio.evaluate((element: HTMLAudioElement) => element.paused),
      ).toBe(true);
      const first = await read<Schema["DownloadStatus"]>(page, recordingPath);
      expect(
        first.status === "ready" && first.intent_id === firstReceipt.intent_id,
      ).toBe(true);
      expect(
        first.source_id !== null &&
          first.source_id !== undefined &&
          uuid.test(first.source_id),
      ).toBe(true);
      await library.assertRetained(page);
      await page
        .getByRole("navigation", { name: "Main navigation" })
        .getByRole("link", { name: "History", exact: true })
        .click();
      await page.goto(details);
      await expect(section.getByRole("status")).toHaveText(
        "Saved on this server",
      );
      await page.reload();
      await expect(section.getByRole("status")).toHaveText(
        "Saved on this server",
      );
      const reloaded = await read<Schema["DownloadStatus"]>(
        page,
        recordingPath,
      );
      expect(
        reloaded.intent_id === first.intent_id &&
          reloaded.source_id === first.source_id,
      ).toBe(true);
      expect(creations.length).toBe(1);
      expect(browserAudioRequests).toBe(0);

      fixture.failSource(fixture.source, 503);
      const providerReads = fixture.requests.length;
      await page.goto(
        `/library?q=${encodeURIComponent(library.personalTitle)}`,
      );
      await page
        .getByRole("button", {
          name: `Play ${library.personalTitle}`,
          exact: true,
        })
        .click();
      await expect
        .poll(() =>
          audio.evaluate((element: HTMLAudioElement) => element.currentTime),
        )
        .toBeGreaterThan(0.2);
      const playing = await audio.evaluate(
        (element: HTMLAudioElement) => element.src,
      );
      expect(
        new URL(playing).origin === origin &&
          new URL(playing).pathname ===
            `/api/v1/sources/${first.source_id}/audio`,
      ).toBe(true);
      expect(fixture.requests.length).toBe(providerReads);
      await page
        .getByRole("contentinfo", { name: "Audio player", exact: true })
        .getByRole("button", { name: "Pause", exact: true })
        .click();
      await expect
        .poll(() =>
          audio.evaluate((element: HTMLAudioElement) => element.paused),
        )
        .toBe(true);
      await page.goto(details);
      await expect(section.getByRole("status")).toHaveText(
        "Saved on this server",
      );

      const oldPath = `/api/v1/downloads/${first.intent_id}`;
      const removalDraft = page.getByLabel("Track title", { exact: true });
      await removalDraft.fill("Unsent removal title draft");
      let committedRemoval = false;
      await page.route(
        (url) => url.origin === origin && url.pathname === oldPath,
        async (route) => {
          expect(route.request().method()).toBe("DELETE");
          const actual = await route.fetch();
          expect(actual.status()).toBe(204);
          committedRemoval = true;
          await actual.dispose();
          await route.abort("failed");
        },
        { times: 1 },
      );
      await section
        .getByRole("button", { name: "Remove download", exact: true })
        .click();
      await expect(
        section.getByRole("button", { name: "Retry removal", exact: true }),
      ).toBeVisible();
      await expect(removalDraft).toHaveValue("Unsent removal title draft");
      expect(committedRemoval).toBe(true);
      expect((await page.request.get(recordingPath)).status()).toBe(404);
      expect(
        (
          await page.request.get(`/api/v1/sources/${first.source_id}/audio`)
        ).status(),
      ).toBe(404);
      await library.assertRetained(page);

      fixture.failSource(fixture.source, 0);
      second = await context.newPage();
      second.on("pageerror", () => pageErrors++);
      await second.goto(details);
      const secondSection = second.getByRole("region", {
        name: "Download",
        exact: true,
      });
      await expect(
        secondSection.getByRole("button", { name: "Download", exact: true }),
      ).toBeEnabled();
      await secondSection
        .getByRole("button", { name: "Download", exact: true })
        .click();
      await expect(secondSection.getByRole("status")).toHaveText(
        "Saved on this server",
        { timeout: 30_000 },
      );
      const fresh = await read<Schema["DownloadStatus"]>(second, recordingPath);
      expect(
        fresh.status === "ready" &&
          fresh.intent_id !== first.intent_id &&
          fresh.source_id !== first.source_id,
      ).toBe(true);
      expect(
        uuid.test(fresh.intent_id) &&
          typeof fresh.source_id === "string" &&
          uuid.test(fresh.source_id),
      ).toBe(true);
      expect(await second.locator("audio").getAttribute("src")).toBeNull();
      expect(
        await second
          .locator("audio")
          .evaluate((element: HTMLAudioElement) => element.paused),
      ).toBe(true);
      expect(creations.length).toBe(2);
      expect(creations[0] !== creations[1]).toBe(true);
      await section
        .getByRole("button", { name: "Retry removal", exact: true })
        .click();
      await expect(section.getByRole("status")).toHaveText(
        "Saved on this server",
      );
      await expect(removalDraft).toHaveValue("Unsent removal title draft");
      const retained = await read<Schema["DownloadStatus"]>(
        page,
        recordingPath,
      );
      expect(
        retained.intent_id === fresh.intent_id &&
          retained.source_id === fresh.source_id &&
          retained.status === "ready",
      ).toBe(true);
      expect(removals.length).toBe(2);
      expect(removals.every((path) => path === oldPath)).toBe(true);
      await expect(
        section.getByRole("button", { name: "Remove download", exact: true }),
      ).toBeEnabled();
      await section
        .getByRole("button", { name: "Remove download", exact: true })
        .click();
      await expect(
        section.getByRole("button", { name: "Download", exact: true }),
      ).toBeEnabled();
      await expect(removalDraft).toHaveValue("Unsent removal title draft");
      expect((await page.request.get(recordingPath)).status()).toBe(404);
      expect(
        (
          await page.request.get(`/api/v1/sources/${fresh.source_id}/audio`)
        ).status(),
      ).toBe(404);
      expect(removals.length).toBe(3);
      expect(removals[2] === `/api/v1/downloads/${fresh.intent_id}`).toBe(true);
      expect(creations.length).toBe(2);
      await library.assertRetained(page);
      expect(pageErrors).toBe(0);
    } finally {
      if (second) await second.close();
      await fixture.close();
    }
  });
});
