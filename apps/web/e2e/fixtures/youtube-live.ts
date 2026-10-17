import { expect, type Locator, type Page } from "@playwright/test";

export function liveYouTubeConfiguration() {
  const raw = process.env.HITO_YOUTUBE_LIVE_BASE_URL;
  const video = process.env.HITO_YOUTUBE_LIVE_VIDEO_ID;
  if (!raw || !video || !/^[A-Za-z0-9_-]{11}$/.test(video))
    throw new Error(
      "The opted-in live test requires its explicit service and canonical video configuration",
    );
  let service: URL;
  try {
    service = new URL(raw);
  } catch {
    throw new Error("The isolated YouTube service configuration is invalid");
  }
  if (
    service.protocol !== "http:" ||
    !["plugin-youtube", "plugin-youtube-live", "hito-youtube-live"].includes(
      service.hostname,
    ) ||
    service.port !== "8093" ||
    service.username ||
    service.password ||
    service.search ||
    service.hash ||
    service.pathname !== "/"
  )
    throw new Error(
      "The live test requires an approved isolated YouTube service alias on its fixed port",
    );
  return { address: service.href, video };
}

export async function signInLiveAccount(page: Page) {
  await page.goto("/library");
  await page.getByLabel("Username", { exact: true }).fill("admin");
  await page
    .getByLabel("Password", { exact: true })
    .fill("docker browser test password");
  const setup = await page
    .getByRole("button", { name: "Create administrator", exact: true })
    .count();
  await page
    .getByRole("button", {
      name: setup ? "Create administrator" : "Sign in",
      exact: true,
    })
    .click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
}

export async function liveAudioState(page: Page) {
  return page
    .locator("audio[data-hito-player]")
    .evaluate((audio: HTMLAudioElement) => ({
      time: audio.currentTime,
      duration: audio.duration,
      ready: audio.readyState,
      paused: audio.paused,
      seeking: audio.seeking,
      error: audio.error?.code ?? 0,
      source: audio.currentSrc || audio.src,
    }));
}

export async function liveMediaDiagnostics(page: Page) {
  return page.evaluate(() => {
    const audio = document.querySelector<HTMLAudioElement>(
      "audio[data-hito-player]",
    );
    let sameOrigin = 0;
    let canonical = 0;
    const source = audio?.currentSrc || audio?.src || "";
    if (source) {
      try {
        const url = new URL(source);
        sameOrigin = Number(url.origin === location.origin);
        canonical = Number(
          sameOrigin === 1 &&
            !url.search &&
            !url.hash &&
            /^\/api\/v1\/sources\/[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}\/audio$/.test(
              url.pathname,
            ),
        );
      } catch {
        /* Report only numeric source-validity flags. */
      }
    }
    const buttons = Array.from(
      document.querySelectorAll<HTMLButtonElement>("button.row-play"),
    );
    const errorMessage = audio?.error?.message.toLowerCase() ?? "";
    return {
      audio_present: Number(audio !== null),
      current_time:
        audio && Number.isFinite(audio.currentTime) ? audio.currentTime : 0,
      duration: audio && Number.isFinite(audio.duration) ? audio.duration : 0,
      duration_finite: Number(
        audio !== null && Number.isFinite(audio.duration),
      ),
      ready_state: audio?.readyState ?? 0,
      network_state: audio?.networkState ?? 0,
      paused: Number(audio?.paused ?? true),
      seeking: Number(audio?.seeking ?? false),
      media_error: audio?.error?.code ?? 0,
      error_mentions_audio_stream: Number(
        /audio[\s_-]*stream/.test(errorMessage),
      ),
      error_mentions_cubeb: Number(errorMessage.includes("cubeb")),
      error_mentions_pulse: Number(errorMessage.includes("pulse")),
      error_mentions_alsa: Number(errorMessage.includes("alsa")),
      error_mentions_decoder: Number(errorMessage.includes("decod")),
      error_mentions_ffmpeg: Number(errorMessage.includes("ffmpeg")),
      error_mentions_gmp: Number(errorMessage.includes("gmp")),
      source_present: Number(source.length > 0),
      source_same_origin: sameOrigin,
      source_canonical: canonical,
      row_play_buttons: buttons.length,
      row_play_enabled: buttons.filter((button) => !button.disabled).length,
      row_play_visible: buttons.filter((button) => {
        const box = button.getBoundingClientRect();
        return box.width > 0 && box.height > 0;
      }).length,
      dock_play_buttons: document.querySelectorAll(
        '.player button[aria-label="Play"]',
      ).length,
      dock_pause_buttons: document.querySelectorAll(
        '.player button[aria-label="Pause"]',
      ).length,
    };
  });
}

export async function liveControlBounds(
  page: Page,
  control: Locator,
  outsideDock: boolean,
) {
  for (const viewport of [
    { width: 1440, height: 1000 },
    { width: 390, height: 844 },
  ]) {
    await page.setViewportSize(viewport);
    await control.evaluate((element) =>
      element.scrollIntoView({ block: "center", inline: "center" }),
    );
    await expect(control).toBeVisible();
    const bounds = await control.boundingBox();
    const dock = await page
      .getByRole("contentinfo", { name: "Audio player" })
      .boundingBox();
    expect(
      Boolean(
        bounds &&
          dock &&
          bounds.x >= 0 &&
          bounds.y >= 0 &&
          bounds.x + bounds.width <= viewport.width &&
          bounds.y + bounds.height <= (outsideDock ? dock.y : viewport.height),
      ),
    ).toBe(true);
    expect(
      await control.evaluate((element) => {
        const box = element.getBoundingClientRect();
        const hit = document.elementFromPoint(
          box.x + box.width / 2,
          box.y + box.height / 2,
        );
        return hit !== null && element.contains(hit);
      }),
    ).toBe(true);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      ),
    ).toBe(false);
  }
  await page.setViewportSize({ width: 1440, height: 1000 });
}
