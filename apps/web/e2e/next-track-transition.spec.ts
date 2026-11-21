import { chromium, expect, test, webkit, type Page } from "@playwright/test";
import { adminSessionPath } from "./fixtures/admin-session";
import { providerFixture } from "./fixtures/provider";
import type { Schema } from "../src/shared/schema";

type Transition = {
  first: string | null;
  endedAt: number | null;
  ended: number | null;
  playing: number | null;
  audible: number | null;
  next: string | null;
};

const engines = { chromium, webkit };
const delays = [0, 1500, 4000];

function watchTransition(page: Page) {
  return page.evaluate(() => {
    const transition: Transition = {
      first: null,
      endedAt: null,
      ended: null,
      playing: null,
      audible: null,
      next: null,
    };
    (window as unknown as { transition: Transition }).transition = transition;
    const source = (event: Event) =>
      (event.target as HTMLAudioElement).getAttribute("src");
    const listen = () => {
      const player = document.querySelector<HTMLAudioElement>(
        "audio[data-hito-player]",
      );
      if (
        player &&
        player.getAttribute("src") !== transition.first &&
        !player.paused &&
        player.currentTime > 0
      ) {
        transition.audible = performance.now();
        transition.next = player.getAttribute("src");
      } else requestAnimationFrame(listen);
    };
    document.addEventListener(
      "ended",
      (event) => {
        if (transition.ended !== null || source(event) !== transition.first)
          return;
        transition.endedAt = Date.now();
        transition.ended = performance.now();
        requestAnimationFrame(listen);
      },
      true,
    );
    document.addEventListener(
      "playing",
      (event) => {
        if (
          transition.ended !== null &&
          transition.playing === null &&
          source(event) !== transition.first
        )
          transition.playing = performance.now();
      },
      true,
    );
  });
}

const transition = (page: Page) =>
  page.evaluate(
    () => (window as unknown as { transition: Transition }).transition,
  );

const position = (page: Page) =>
  page
    .locator("audio[data-hito-player]")
    .evaluate((element: HTMLAudioElement) =>
      element.paused ? 0 : element.currentTime,
    );

for (const [name, engine] of Object.entries(engines))
  for (const delay of delays)
    test(`the next track plays from its buffered element in ${name} with a ${delay} ms provider delay`, async ({
      baseURL,
    }, testInfo) => {
      test.setTimeout(60_000 + 6 * delay);
      const prefix = `Transition ${name} delay${delay}`;
      const fixture = await providerFixture(prefix);
      const browser = await engine.launch({ args: [] });
      const errors: string[] = [];
      let cleanup: (() => Promise<void>) | null = null;
      try {
        const context = await browser.newContext({
          baseURL,
          storageState: adminSessionPath,
          serviceWorkers: "block",
          viewport: { width: 1440, height: 1000 },
        });
        const page = await context.newPage();
        page.on("pageerror", (error) => errors.push(error.message));
        const headers = {
          "x-hito-request": "1",
          origin: new URL(baseURL!).origin,
        };
        const offered = await page.request.post("/api/v1/plugins/services", {
          headers,
          data: {
            name: `${prefix} service`,
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
        const saved: Schema<"Recording">[] = [];
        for (const item of [fixture.recording, fixture.second]) {
          const response = await page.request.post(
            `/api/v1/plugins/${connection.id}/recordings`,
            {
              headers,
              data: {
                installation_revision: connection.revision,
                provider_recording_id: item.id,
              } satisfies Schema<"AdoptProviderRecording">,
            },
          );
          expect(response.status()).toBe(200);
          saved.push(await response.json());
        }
        fixture.delayAudio(delay);
        const nextRequests = () =>
          fixture.requests.filter(
            (request) => request.source === fixture.secondSource,
          );

        await page.goto(`/library?q=${encodeURIComponent(prefix)}&sort=title`);
        await watchTransition(page);
        await page
          .getByRole("button", { name: `Play ${saved[0].title}`, exact: true })
          .click();
        await expect
          .poll(() => position(page), { timeout: 15_000 + 2 * delay })
          .toBeGreaterThan(0.2);
        const first = await page
          .locator("audio[data-hito-player]")
          .evaluate((element: HTMLAudioElement) => {
            (window as unknown as { transition: Transition }).transition.first =
              element.getAttribute("src");
            return element.getAttribute("src");
          });
        await expect
          .poll(
            () =>
              page.evaluate(
                () =>
                  (document.querySelector<HTMLAudioElement>(
                    "audio[data-hito-preload]",
                  )?.readyState ?? 0) >= 4,
              ),
            { timeout: 15_000 + 2 * delay },
          )
          .toBe(true);
        expect(nextRequests().length).toBeGreaterThan(0);

        await page
          .locator("audio[data-hito-player]")
          .evaluate((element: HTMLAudioElement) => {
            element.currentTime = Math.max(0, element.duration - 1);
          });
        await expect
          .poll(async () => (await transition(page)).audible, {
            timeout: 15_000 + 2 * delay,
          })
          .not.toBeNull();
        await expect.poll(() => position(page)).toBeGreaterThan(1);
        const measured = await transition(page);
        const requests = nextRequests();
        const result = {
          engine: name,
          delay_ms: delay,
          ended_to_playing_ms: Math.round(measured.playing! - measured.ended!),
          ended_to_audible_ms: Math.round(measured.audible! - measured.ended!),
          next_requests: requests.length,
          next_requests_from_start: requests.filter(
            (request) => !request.range || request.range === "bytes=0-",
          ).length,
          next_requests_after_ended: requests.filter(
            (request) => request.at >= measured.endedAt!,
          ).length,
        };
        testInfo.annotations.push({
          type: "transition",
          description: JSON.stringify(result),
        });
        console.log(`next-track-transition ${JSON.stringify(result)}`);

        expect(measured.next).not.toBe(first);
        expect(
          await page.evaluate(
            () => document.querySelectorAll("audio[data-hito-player]").length,
          ),
        ).toBe(1);
        expect(result.next_requests_from_start).toBe(1);
        expect(result.next_requests_after_ended).toBe(0);
        expect(result.ended_to_audible_ms).toBeLessThan(500);
        expect(errors).toEqual([]);
      } finally {
        try {
          await cleanup?.();
        } finally {
          await browser.close();
          await fixture.close();
        }
      }
    });
