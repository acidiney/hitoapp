import type { BrowserContext, Page } from "@playwright/test";
import { captureConsolePrivacy } from "./console-privacy";

export const audioPath =
  /^\/api\/v1\/sources\/[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}\/audio$/;
export const livePassword = "disposable live RSS browser password";

export function liveFeed() {
  const raw =
    process.env.HITO_PODCAST_LIVE_FEED ??
    "https://rustacean-station.org/podcast.rss";
  const feed = new URL(raw);
  if (
    feed.href !== raw ||
    feed.protocol !== "https:" ||
    feed.port ||
    feed.username ||
    feed.password ||
    feed.search ||
    feed.hash ||
    !/^[a-z0-9.-]+\.[a-z]{2,}$/i.test(feed.hostname) ||
    /\.(localhost|local|internal)$/i.test(feed.hostname)
  )
    throw new Error("A canonical public HTTPS feed is required");
  return feed.href;
}

export function liveReader(page: Page) {
  let requests = 0;
  return async <T>(path: string): Promise<T> => {
    if (++requests > 80) throw new Error("The public read budget was exceeded");
    const response = await page.request.get(path, { timeout: 10_000 });
    if (response.status() !== 200) throw new Error("A public read failed");
    const body = await response.body();
    if (body.length > 524_288)
      throw new Error("A public response exceeded its bound");
    return JSON.parse(body.toString()) as T;
  };
}

export async function liveAudio(page: Page) {
  return page
    .locator("audio[data-hito-player]")
    .evaluate((audio: HTMLAudioElement) => {
      let sourceId = "";
      try {
        const url = new URL(audio.currentSrc);
        const match =
          /^\/api\/v1\/sources\/([a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12})\/audio$/.exec(
            url.pathname,
          );
        if (url.origin === location.origin && !url.search && !url.hash && match)
          sourceId = match[1];
      } catch {}
      return {
        time: audio.currentTime,
        duration: audio.duration,
        paused: audio.paused,
        seeking: audio.seeking,
        error: audio.error?.code ?? 0,
        sourceId,
      };
    });
}

export async function watchLiveAudio(page: Page) {
  const observation = await page.evaluateHandle(() => {
    const audio = document.querySelector<HTMLAudioElement>(
      "audio[data-hito-player]",
    );
    if (!audio) throw new Error("The live audio element is absent");
    const counts = { pause: 0, emptied: 0, ended: 0, error: 0 };
    const detach: (() => void)[] = [];
    for (const name of ["pause", "emptied", "ended", "error"] as const) {
      const listener = () => {
        counts[name]++;
      };
      audio.addEventListener(name, listener);
      detach.push(() => audio.removeEventListener(name, listener));
    }
    return {
      snapshot: () => ({
        same:
          audio.isConnected &&
          document.querySelector("audio[data-hito-player]") === audio,
        ...counts,
      }),
      detach: () => {
        for (const remove of detach) remove();
      },
    };
  });
  return {
    snapshot: () => observation.evaluate((state) => state.snapshot()),
    async uninterrupted() {
      const state = await observation.evaluate((value) => value.snapshot());
      return (
        state.same &&
        state.pause + state.emptied + state.ended + state.error === 0
      );
    },
    async dispose() {
      try {
        await observation.evaluate((state) => state.detach());
      } finally {
        await observation.dispose();
      }
    },
  };
}

export async function liveConsolePrivacy(context: BrowserContext) {
  const captured: string[] = [];
  const forbidden = [livePassword];
  const pending = new Set<Promise<void>>();
  let size = 0;
  let failed = false;
  await captureConsolePrivacy(
    context,
    (text) => {
      size += Buffer.byteLength(text);
      if (size > 524_288) failed = true;
      else captured.push(text);
    },
    () => {
      failed = true;
    },
    pending,
  );
  return {
    forbid(...values: (string | null | undefined)[]) {
      for (const value of values) if (value) forbidden.push(value);
    },
    async clean() {
      await Promise.allSettled([...pending]);
      return (
        !failed &&
        captured.every((text) =>
          forbidden.every((value) => !text.includes(value)),
        )
      );
    },
  };
}
