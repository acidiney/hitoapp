import { expect, test } from "@playwright/test";
import { providerFixture } from "./fixtures/provider";
import type { Schema } from "../src/shared/schema";

test("play-all queues every library page, then a reorder changes the next transition without restarting audio", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(90_000);
  const fixture = await providerFixture("Order", true);
  const errors: string[] = [];
  const pageRequests: string[] = [];
  let downloads = 0;
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (request) => {
    const url = new URL(request.url());
    if (url.pathname === "/api/v1/recordings" && url.searchParams.has("offset"))
      pageRequests.push(`${url.search}`);
    if (request.method() === "POST" && url.pathname.endsWith("/download"))
      downloads++;
  });
  let cleanup: (() => Promise<void>) | null = null;
  try {
    await page.goto("/library");
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    const headers = { "x-hito-request": "1", origin: new URL(baseURL!).origin };
    const offered = await page.request.post("/api/v1/plugins/services", {
      headers,
      data: {
        name: "Queue order service",
        base_url: fixture.address,
      } satisfies Schema<"OfferPluginService">,
    });
    expect(offered.status()).toBe(201);
    const service: Schema<"PluginService"> = await offered.json();
    const installed = await page.request.post("/api/v1/plugins", {
      headers,
      data: {
        service_id: service.id,
        service_revision: service.revision,
        bearer_token: fixture.token,
      } satisfies Schema<"InstallPlugin">,
    });
    expect(installed.status()).toBe(201);
    const connection: Schema<"PluginInstallation"> = await installed.json();
    cleanup = async () => {
      const removed = await page.request.delete(
        `/api/v1/plugins/${connection.id}`,
        { headers, data: { revision: connection.revision } },
      );
      expect(removed.ok()).toBe(true);
    };
    const providerIds = [
      fixture.recording.id,
      fixture.second.id,
      ...Array.from(
        { length: 5 },
        (_, index) => `provider/private/catalog-${index}`,
      ),
    ];
    for (const id of providerIds) {
      const adopted = await page.request.post(
        `/api/v1/plugins/${connection.id}/recordings`,
        {
          headers,
          data: {
            installation_revision: connection.revision,
            provider_recording_id: id,
          } satisfies Schema<"AdoptProviderRecording">,
        },
      );
      expect(adopted.status()).toBe(200);
    }

    await page.goto("/library?q=Order&limit=3");
    await expect(page.getByRole("button", { name: /^Play Order/ })).toHaveCount(
      3,
    );
    await expect(
      page.getByRole("navigation", { name: "Result pages" }),
    ).toContainText("1 / 3");
    expect(pageRequests).toEqual([]);

    await page
      .getByRole("button", { name: "Play tracks", exact: true })
      .click();
    const audio = page.locator("audio[data-hito-player]");
    await expect
      .poll(() =>
        audio.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(0.2);
    await expect.poll(() => pageRequests.length).toBe(1);
    expect(new URLSearchParams(pageRequests[0]).get("q")).toBe("Order");
    expect(new URLSearchParams(pageRequests[0]).get("offset")).toBe("3");

    await page.getByRole("button", { name: "Open queue", exact: true }).click();
    await expect(page).toHaveURL(/\/queue$/);
    const now = page.getByRole("region", { name: "Now playing", exact: true });
    const rows = page.locator("ol.queue-tracks > li");
    const titles = () => rows.locator("strong").allTextContents();
    await expect(now.locator("strong")).toHaveText("Order catalog fixture 1");
    await expect(rows).toHaveCount(6);
    expect(await titles()).toEqual([
      "Order catalog fixture 2",
      "Order catalog fixture 3",
      "Order catalog fixture 4",
      "Order catalog fixture 5",
      "Order owned fixture",
      "Order second fixture",
    ]);

    const playing = await audio.evaluate((element: HTMLAudioElement) => ({
      source: element.src,
      time: element.currentTime,
    }));
    const up = page.getByRole("button", {
      name: "Move Order second fixture up",
      exact: true,
    });
    await up.focus();
    for (let step = 0; step < 5; step++) {
      await page.keyboard.press("Enter");
      await expect(rows.nth(4 - step).locator("strong")).toHaveText(
        "Order second fixture",
      );
    }
    expect(await titles()).toEqual([
      "Order second fixture",
      "Order catalog fixture 2",
      "Order catalog fixture 3",
      "Order catalog fixture 4",
      "Order catalog fixture 5",
      "Order owned fixture",
    ]);
    await expect(
      rows.nth(0).getByRole("button", { name: /^Move .* down$/ }),
    ).toBeFocused();

    await rows
      .nth(5)
      .getByRole("button", { name: "Drag Order owned fixture" })
      .dragTo(rows.nth(1), { targetPosition: { x: 40, y: 4 } });
    await expect
      .poll(titles)
      .toEqual([
        "Order second fixture",
        "Order owned fixture",
        "Order catalog fixture 2",
        "Order catalog fixture 3",
        "Order catalog fixture 4",
        "Order catalog fixture 5",
      ]);

    const after = await audio.evaluate((element: HTMLAudioElement) => ({
      source: element.src,
      time: element.currentTime,
      paused: element.paused,
    }));
    expect(after.source).toBe(playing.source);
    expect(after.time).toBeGreaterThanOrEqual(playing.time);
    expect(after.paused).toBe(false);

    await audio.evaluate((element: HTMLAudioElement) => {
      element.currentTime = element.duration - 1;
    });
    await expect(now.locator("strong")).toHaveText("Order second fixture", {
      timeout: 20_000,
    });
    await expect
      .poll(() => audio.evaluate((element: HTMLAudioElement) => element.src))
      .not.toBe(playing.source);
    await expect
      .poll(() =>
        audio.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(0);
    await audio.evaluate((element: HTMLAudioElement) => {
      element.currentTime = element.duration - 1;
    });
    await expect(now.locator("strong")).toHaveText("Order owned fixture", {
      timeout: 20_000,
    });
    expect(downloads).toBe(0);
    expect(errors).toEqual([]);
  } finally {
    await cleanup?.();
    await fixture.close();
  }
});
