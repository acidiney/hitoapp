import { signOutFromShell } from "./fixtures/account";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import { captureConsolePrivacy } from "./fixtures/console-privacy";
import { mediaSessionProvider } from "./fixtures/media-session-provider";
import {
  observeMediaSession,
  systemAction,
} from "./fixtures/media-session-browser";
import { prepareMediaSessionLibrary } from "./fixtures/media-session-library";

test.use({
  launchOptions: { args: [] },
  trace: "off",
  screenshot: "off",
  video: "off",
});
test.skip(
  process.env.HITO_MEDIA_SESSION_E2E !== "1",
  "Requires a fresh disposable media-session host sharing the fixture loopback network",
);

async function audioState(page: Page, expectedSource?: string) {
  return page.locator("audio[data-hito-player]").evaluate(
    (audio: HTMLAudioElement, expectedSource) => ({
      playing: !audio.paused && !audio.ended && audio.error === null,
      position: audio.currentTime,
      duration: audio.duration,
      source: /^\/api\/v1\/sources\/[0-9a-f-]+\/audio$/.test(
        new URL(audio.src || location.href).pathname,
      ),
      exactSource:
        expectedSource !== undefined &&
        audio.src ===
          new URL(`/api/v1/sources/${expectedSource}/audio`, location.href)
            .href,
    }),
    expectedSource,
  );
}

async function cleared(page: Page) {
  return page.evaluate(
    () =>
      navigator.mediaSession.metadata === null &&
      navigator.mediaSession.playbackState === "none" &&
      window.__hitoOwnedMediaSession?.handlerCount() === 0 &&
      window.__hitoOwnedMediaSession.position() === null,
  );
}

test("real browser media session follows owned playback, verified artwork, system controls, and logout", async ({
  page,
  context,
  baseURL,
  browserName,
}, info) => {
  test.setTimeout(75_000);
  page.setDefaultTimeout(8000);
  page.setDefaultNavigationTimeout(8000);
  let phase = "configuration";
  let failedPhase: string | null = null;
  let provider: Awaited<ReturnType<typeof mediaSessionProvider>> | undefined;
  let releaseNavigation: (() => void) | undefined;
  let heldRoute: Route | undefined;
  let routeTimer: ReturnType<typeof setTimeout> | undefined;
  let navigationExpired = false;
  const pendingConsole = new Set<Promise<void>>();
  const counts = {
    foreign: 0,
    pageErrors: 0,
    downloads: 0,
    choices: 0,
    resolution: 0,
    leaked: 0,
    privacy: 0,
    textProbe: 0,
    snapshotProbe: 0,
  };
  let supported = false;
  let decodedArtwork = false;
  let played = false;
  let logoutCleared = false;
  try {
    if (!baseURL || process.env.PLAYWRIGHT_NO_COPY_PROMPT !== "1")
      throw new Error(
        "The isolated media session privacy configuration is required",
      );
    const target = new URL(baseURL);
    if (
      target.protocol !== "http:" ||
      !["localhost", "127.0.0.1"].includes(target.hostname) ||
      target.port !== "8080" ||
      target.pathname !== "/" ||
      target.username ||
      target.password ||
      target.search ||
      target.hash
    )
      throw new Error("The media session server must be isolated on loopback");
    const origin = target.origin;
    provider = await mediaSessionProvider();
    const fixture = provider;
    const observations = await observeMediaSession(context);
    await captureConsolePrivacy(
      context,
      (text, capture) => {
        if (text.includes("owned-media-session-console-probe"))
          counts[capture === "text" ? "textProbe" : "snapshotProbe"]++;
        if (
          [
            ...fixture.privateValues,
            fixture.address,
            "docker browser test password",
          ].some((value) => text.includes(value))
        )
          counts.leaked++;
      },
      () => counts.privacy++,
      pendingConsole,
    );
    await context.addInitScript(() =>
      localStorage.setItem("hito-log-level", "debug"),
    );
    page.on("pageerror", () => counts.pageErrors++);
    await context.route("**/*", async (route) => {
      const url = new URL(route.request().url());
      if (url.protocol.startsWith("http") && url.origin !== origin) {
        counts.foreign++;
        await route.abort();
      } else await route.continue();
    });
    context.on("request", (request) => {
      const url = new URL(request.url());
      if (url.origin !== origin || ["GET", "HEAD"].includes(request.method()))
        return;
      if (/\/downloads?(?:\/|$)/.test(url.pathname)) counts.downloads++;
      if (/\/sources\/(review|attach)$|\/source-choice$/.test(url.pathname))
        counts.choices++;
      if (/\/sources\/(resolve|refresh)$/.test(url.pathname))
        counts.resolution++;
    });
    phase = "browser_support";
    const setup = await page.request.get("/api/v1/setup");
    expect(setup.status()).toBe(200);
    expect(((await setup.json()) as Schema<"SetupStatus">).needs_setup).toBe(
      true,
    );
    await page.goto("/library");
    supported = await page.evaluate(
      () =>
        !!navigator.mediaSession &&
        typeof MediaMetadata === "function" &&
        typeof navigator.mediaSession.setActionHandler === "function" &&
        typeof navigator.mediaSession.setPositionState === "function",
    );
    test.skip(
      !supported,
      `The ${browserName} engine does not expose native Media Session metadata, actions, and position APIs`,
    );
    expect(
      await page.evaluate(
        () => window.__hitoOwnedMediaSession?.supported === true,
      ),
    ).toBe(true);
    await page.evaluate(() =>
      console.info("owned-media-session-console-probe"),
    );
    phase = "public_setup";
    const { records, expectedSources } = await prepareMediaSessionLibrary(
      page,
      origin,
      fixture,
    );
    phase = "verified_cover";
    const covers = page
      .getByRole("table", { name: "Playlist entries", exact: true })
      .locator(".artwork img");
    await expect
      .poll(() =>
        covers.evaluateAll(
          (images) =>
            images.length === 2 &&
            images.every(
              (image) =>
                image instanceof HTMLImageElement &&
                image.complete &&
                image.naturalWidth > 0,
            ),
        ),
      )
      .toBe(true);
    for (const record of records) {
      const image = await page.request.get(
        `/api/v1/recordings/${record.id}/artwork`,
      );
      expect(image.status()).toBe(200);
      expect(
        image.headers()["cache-control"] === "private, no-store" &&
          (await image.body()).equals(fixture.cover),
      ).toBe(true);
    }
    expect(fixture.counts.audio).toBe(0);
    phase = "user_play";
    await page
      .getByRole("button", { name: "Play playlist", exact: true })
      .click();
    await expect
      .poll(async () => {
        const state = await audioState(page, expectedSources[0]);
        return (
          state.playing &&
          state.source &&
          state.exactSource &&
          state.position > 0.2 &&
          Math.abs(state.duration - 60) < 0.1
        );
      })
      .toBe(true);
    played = true;
    const metadataMatches = (index: number) =>
      page.evaluate(
        ({ record, width, height }) => {
          const metadata = navigator.mediaSession.metadata;
          const artwork = metadata?.artwork[0];
          return (
            metadata?.title === record.title &&
            metadata.artist === record.artist &&
            metadata.album === "" &&
            artwork?.src ===
              new URL(`/api/v1/recordings/${record.id}/artwork`, location.href)
                .href &&
            artwork.sizes === `${width}x${height}`
          );
        },
        {
          record: records[index],
          width: fixture.width,
          height: fixture.height,
        },
      );
    await expect.poll(() => metadataMatches(0)).toBe(true);
    expect(
      await page.evaluate(
        () =>
          navigator.mediaSession.playbackState === "playing" &&
          window.__hitoOwnedMediaSession?.position()?.duration === 60,
      ),
    ).toBe(true);
    decodedArtwork = await page.evaluate(
      () =>
        new Promise<boolean>((resolve) => {
          const image = new Image();
          const timer = setTimeout(() => finish(false), 5000);
          function finish(ready: boolean) {
            clearTimeout(timer);
            image.onload = null;
            image.onerror = null;
            image.removeAttribute("src");
            resolve(ready);
          }
          image.onload = () =>
            finish(image.naturalWidth > 0 && image.naturalHeight > 0);
          image.onerror = () => finish(false);
          image.src = navigator.mediaSession.metadata!.artwork[0].src;
        }),
    );
    expect(decodedArtwork).toBe(true);
    phase = "system_controls";
    await systemAction(page, "pause");
    await expect.poll(async () => (await audioState(page)).playing).toBe(false);
    await expect
      .poll(
        () =>
          page.evaluate(
            () => navigator.mediaSession.playbackState === "paused",
          ),
        { timeout: 8000 },
      )
      .toBe(true);
    await systemAction(page, "seekto", { seekTime: 6 });
    expect(Math.abs((await audioState(page)).position - 6) < 0.1).toBe(true);
    await systemAction(page, "play");
    await expect
      .poll(async () => {
        const state = await audioState(page);
        return state.playing && state.position > 6.1;
      })
      .toBe(true);
    await systemAction(page, "nexttrack");
    await expect.poll(() => metadataMatches(1)).toBe(true);
    await expect
      .poll(async () => {
        const state = await audioState(page, expectedSources[1]);
        return state.playing && state.exactSource;
      })
      .toBe(true);
    await systemAction(page, "pause");
    await systemAction(page, "seekto", { seekTime: 0 });
    await systemAction(page, "previoustrack");
    await expect.poll(() => metadataMatches(0)).toBe(true);
    await expect
      .poll(async () => {
        const state = await audioState(page, expectedSources[0]);
        return state.playing && state.exactSource;
      })
      .toBe(true);
    await page.evaluate(() => window.__hitoOwnedMediaSession?.remember());
    await systemAction(page, "stop");
    expect(await cleared(page)).toBe(true);
    await page.evaluate(() => window.__hitoOwnedMediaSession?.invokeHeld());
    expect(await cleared(page)).toBe(true);
    phase = "logout_cleanup";
    await page
      .getByRole("button", { name: "Play playlist", exact: true })
      .click();
    await expect.poll(() => metadataMatches(0)).toBe(true);
    await expect
      .poll(async () => {
        const state = await audioState(page, expectedSources[0]);
        return state.playing && state.exactSource;
      })
      .toBe(true);
    await page.evaluate(() => window.__hitoOwnedMediaSession?.remember());
    const logoutObservation = await observations.armLogout(page);
    let entered!: () => void;
    const navigating = new Promise<void>((resolve) => {
      entered = resolve;
    });
    const gate = new Promise<void>((resolve) => {
      releaseNavigation = resolve;
    });
    await page.route(
      (url) => url.href === `${origin}/`,
      async (route) => {
        heldRoute = route;
        routeTimer = setTimeout(() => {
          navigationExpired = true;
          releaseNavigation?.();
        }, 8000);
        entered();
        await gate;
        clearTimeout(routeTimer);
        await route.continue();
        heldRoute = undefined;
      },
    );
    const signingOut = signOutFromShell(page, { noWaitAfter: true });
    void signingOut.catch(() => {});
    await navigating;
    const logoutEvidence = await logoutObservation.result;
    logoutCleared =
      logoutEvidence.armed &&
      logoutEvidence.beforeCleared &&
      logoutEvidence.heldPresent &&
      logoutEvidence.heldInvoked &&
      logoutEvidence.afterCleared;
    expect(logoutCleared).toBe(true);
    expect(observations.failures()).toBe(0);
    expect(
      (
        await page.request.get(`/api/v1/recordings/${records[0].id}/artwork`)
      ).status(),
    ).toBe(401);
    releaseNavigation?.();
    await signingOut;
    await expect(
      page.getByRole("button", { name: "Sign in", exact: true }),
    ).toBeVisible();
    expect(
      await page.evaluate(() => navigator.mediaSession.metadata === null),
    ).toBe(true);
    phase = "preservation_and_privacy";
    expect(observations.failures()).toBe(0);
    await Promise.all([...pendingConsole]);
    expect(counts.textProbe > 0 && counts.snapshotProbe > 0).toBe(true);
    expect(
      counts.foreign +
        counts.pageErrors +
        counts.downloads +
        counts.choices +
        counts.resolution +
        counts.leaked +
        counts.privacy,
    ).toBe(0);
    expect(
      fixture.counts.protocol +
        fixture.counts.unexpected +
        fixture.counts.handler,
    ).toBe(0);
    expect(fixture.counts.audio > 0 && fixture.counts.artwork === 2).toBe(true);
    expect(navigationExpired).toBe(false);
    phase = "complete";
  } catch (error) {
    failedPhase = phase;
    throw error;
  } finally {
    releaseNavigation?.();
    clearTimeout(routeTimer);
    if (heldRoute) await heldRoute.abort().catch(() => {});
    if (provider) await provider.close();
    await info.attach("browser-media-session-evidence", {
      contentType: "application/json",
      body: JSON.stringify({
        browser: browserName,
        phase,
        failedPhase,
        supported,
        played,
        decodedArtwork,
        logoutCleared,
        navigationExpired,
        counts,
        provider: provider?.counts,
      }),
    });
  }
});
