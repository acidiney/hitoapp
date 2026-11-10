import { expect, type Page, type Response } from "@playwright/test";
import { readFile } from "node:fs/promises";
import type { Schema } from "../../src/shared/schema";

export function dock(page: Page) {
  return page.getByRole("contentinfo", { name: "Audio player", exact: true });
}

export function seek(page: Page) {
  return dock(page).getByRole("slider", { name: "Seek", exact: true });
}

export async function mediaState(page: Page) {
  return page.locator("audio[data-hito-player]").evaluate((element) => {
    const audio = element as HTMLAudioElement;
    const source = audio.currentSrc || audio.src;
    const start = Number(
      new URL(source, location.href).searchParams.get("start_ms") ?? 0,
    );
    return {
      source,
      position: audio.currentTime,
      absolute: start / 1000 + audio.currentTime,
      paused: audio.paused,
      ended: audio.ended,
      error: audio.error?.code ?? null,
      ready: audio.readyState,
      played: audio.played.length,
    };
  });
}

export async function advancing(page: Page) {
  const before = await mediaState(page);
  await expect
    .poll(async () => (await mediaState(page)).position)
    .toBeGreaterThan(before.position + 0.4);
  const after = await mediaState(page);
  expect(after.source).toBe(before.source);
  expect(after.paused).toBe(false);
  expect(after.ended).toBe(false);
  expect(after.error).toBeNull();
  expect(after.ready).toBeGreaterThanOrEqual(2);
  expect(after.played).toBeGreaterThan(0);
  await expect
    .poll(async () => Number(await seek(page).inputValue()))
    .toBeGreaterThanOrEqual(after.absolute - 0.75);
  const position = Number(await seek(page).inputValue());
  expect(Math.abs(position - (await mediaState(page)).absolute)).toBeLessThan(
    0.75,
  );
  await expect(seek(page)).toHaveAttribute("max", "90");
  await expect(dock(page).locator(".seek")).toContainText("1:30");
  return after;
}

export function convertedResponse(
  page: Page,
  quality: "high" | "saver",
  minimum = 0,
  maximum = 0,
) {
  return page.waitForResponse(async (response) => {
    const url = new URL(response.url());
    const start = Number(url.searchParams.get("start_ms") ?? 0);
    const request = response.request();
    const browserAudio =
      request.resourceType() === "media" ||
      (page.context().browser()?.browserType().name() === "webkit" &&
        request.resourceType() === "other" &&
        response.status() !== 0 &&
        (await request.headerValue("sec-fetch-dest")) === "audio");
    return (
      request.method() === "GET" &&
      browserAudio &&
      /\/api\/v1\/sources\/[^/]+\/audio$/.test(url.pathname) &&
      url.searchParams.get("quality") === quality &&
      start >= minimum &&
      start <= maximum
    );
  });
}

export async function verifyConverted(
  page: Page,
  response: Response,
  quality: "high" | "saver",
) {
  const url = new URL(response.url());
  const start = url.searchParams.get("start_ms") ?? "0";
  expect(start).toMatch(/^\d+$/);
  expect(response.status()).toBe(200);
  const headers = response.headers();
  expect(headers["content-type"]).toMatch(/^audio\/aac(?:;|$)/);
  expect(headers["x-hito-stream-quality"]).toBe(quality);
  expect(headers["x-hito-stream-start-ms"]).toBe(start);
  expect(headers["accept-ranges"]).not.toBe("bytes");
  expect(headers["content-range"]).toBeUndefined();
  expect(
    await response.finished(),
    "Converted media body must finish successfully",
  ).toBeNull();
  expect((await response.body()).length).toBeGreaterThan(1024);
  await expect
    .poll(async () => (await mediaState(page)).source)
    .toBe(response.url());

  const head = await page.request.head(`${url.pathname}${url.search}`);
  expect(head.status()).toBe(200);
  expect(head.headers()["content-type"]).toMatch(/^audio\/aac(?:;|$)/);
  expect(head.headers()["x-hito-stream-quality"]).toBe(quality);
  expect(head.headers()["x-hito-stream-start-ms"]).toBe(start);
}

export async function playbackSettings(page: Page) {
  await page.getByRole("link", { name: "Account and settings" }).click();
  await page
    .getByRole("navigation", { name: "Settings sections" })
    .getByRole("link", { name: "Playback", exact: true })
    .click();
  return page.getByRole("combobox", { name: "Streaming quality", exact: true });
}

export async function playFixture(page: Page, title: string) {
  await page
    .getByRole("navigation", { name: "Main navigation" })
    .getByRole("link", { name: "Library", exact: true })
    .click();
  await page
    .getByRole("button", { name: `Play ${title}`, exact: true })
    .click();
  await expect(dock(page)).toContainText(title);
}

export async function connectFixtures(page: Page, baseURL: string) {
  const libraries: Schema<"Library">[] = await (
    await page.request.get("/api/v1/libraries")
  ).json();
  expect(libraries).toHaveLength(1);
  for (const [directory, name] of [
    ["fixture-music", "Music"],
    ["fixture-archive", "Archive"],
  ]) {
    const response = await page.request.post(
      `/api/v1/libraries/${libraries[0].id}/folders`,
      {
        headers: { "x-hito-request": "1", origin: new URL(baseURL).origin },
        data: { directory: { directory_id: directory, components: [] }, name },
      },
    );
    expect([201, 409]).toContain(response.status());
  }
  await expect
    .poll(
      async () => {
        const result: Schema<"RecordingPage"> = await (
          await page.request.get("/api/v1/recordings?limit=10")
        ).json();
        return result.items
          .filter((recording) => /^Fixture (440|660) Hz$/.test(recording.title))
          .map((recording) => recording.duration_ms)
          .sort();
      },
      { timeout: 30_000 },
    )
    .toEqual([90_000, 90_000]);
}

type MediaFailure = {
  source: string;
  code: number | null;
  trusted: boolean;
};

export async function observeQualityErrors(page: Page) {
  const javascript: string[] = [];
  const media: MediaFailure[] = [];
  const unexpectedRequests: string[] = [];
  const convertedMediaRequests: string[] = [];
  page.on("pageerror", (error) => javascript.push(error.message));
  page.on("request", (request) => {
    const url = new URL(request.url());
    const path = url.pathname;
    if (
      request.method() === "GET" &&
      request.resourceType() === "media" &&
      /\/api\/v1\/sources\/[^/]+\/audio$/.test(path) &&
      ["high", "saver"].includes(url.searchParams.get("quality") ?? "")
    ) {
      convertedMediaRequests.push(request.url());
    }
    if (
      request.method() !== "GET" &&
      request.method() !== "HEAD" &&
      (/\/sources\/(?:refresh|resolve)$/.test(path) ||
        /\/downloads?(?:\/|$)/.test(path))
    ) {
      unexpectedRequests.push(`${request.method()} ${path}`);
    }
  });
  await page.exposeFunction(
    "__hitoQualityMediaFailure",
    (failure: MediaFailure) => {
      media.push(failure);
    },
  );
  await page.addInitScript(() => {
    document.addEventListener(
      "error",
      (event) => {
        const audio = event.target;
        if (!(audio instanceof HTMLAudioElement)) return;
        void (
          globalThis as unknown as {
            __hitoQualityMediaFailure(failure: MediaFailure): Promise<void>;
          }
        ).__hitoQualityMediaFailure({
          source: audio.currentSrc || audio.src,
          code: audio.error?.code ?? null,
          trusted: event.isTrusted,
        });
      },
      true,
    );
  });
  return { javascript, media, unexpectedRequests, convertedMediaRequests };
}

export async function ownedFixtureSources(page: Page) {
  const recordings: Schema<"RecordingPage"> = await (
    await page.request.get("/api/v1/recordings?limit=10")
  ).json();
  const fixtures = [];
  for (const [frequency, folder] of [
    [440, "Music"],
    [660, "Archive"],
  ] as const) {
    const recording = recordings.items.find(
      (item) => item.title === `Fixture ${frequency} Hz`,
    );
    expect(recording).toBeDefined();
    const sources: Schema<"Source">[] = await (
      await page.request.get(
        `/api/v1/recordings/${recording!.id}/playback-sources`,
      )
    ).json();
    expect(sources).toHaveLength(1);
    expect(sources[0].provider).toBe("local");
    expect(sources[0].ready).toBe(true);
    const bytes = await readFile(
      `/workspace/fixtures/e2e-generated/${folder}/Fixture ${frequency} Hz.wav`,
    );
    expect(bytes.toString("ascii", 0, 4)).toBe("RIFF");
    expect(bytes.toString("ascii", 8, 12)).toBe("WAVE");
    expect(bytes.readUInt32LE(40) / bytes.readUInt32LE(28)).toBe(90);
    fixtures.push({
      recording: recording!.id,
      path: `/api/v1/sources/${sources[0].id}/audio`,
      bytes,
    });
  }
  return fixtures;
}

export function originalResponse(page: Page, path: string) {
  return page.waitForResponse((response) => {
    const url = new URL(response.url());
    return (
      response.request().method() === "GET" &&
      response.request().resourceType() === "media" &&
      url.pathname === path &&
      url.search === ""
    );
  });
}

export async function verifyOriginal(
  page: Page,
  response: Response,
  fixture: { path: string; bytes: Buffer },
) {
  const url = new URL(response.url());
  expect(url.pathname).toBe(fixture.path);
  expect(url.search).toBe("");
  expect([200, 206]).toContain(response.status());
  expect(response.headers()["content-type"]).toMatch(
    /^audio\/(?:wav|x-wav)(?:;|$)/,
  );
  expect(response.headers()["x-hito-stream-quality"]).toBe("original");
  expect(response.headers()["x-hito-stream-start-ms"]).toBeUndefined();
  await expect
    .poll(async () => (await mediaState(page)).source)
    .toBe(response.url());
  const requestRange = await response.request().headerValue("range");
  const replay = await page.request.get(fixture.path, {
    headers: requestRange ? { range: requestRange } : {},
  });
  expect(replay.status()).toBe(response.status());
  expect(replay.headers()["x-hito-stream-quality"]).toBe("original");
  const contentRange = response.headers()["content-range"];
  expect(replay.headers()["content-range"]).toBe(contentRange);
  const body = await replay.body();
  if (response.status() === 200) expect(body.equals(fixture.bytes)).toBe(true);
  else {
    const range = contentRange?.match(/^bytes (\d+)-(\d+)\/(\d+)$/);
    expect(range).toBeDefined();
    const [start, end, total] = range!.slice(1).map(Number);
    expect(start).toBeGreaterThanOrEqual(0);
    expect(end).toBeGreaterThanOrEqual(start);
    expect(end).toBeLessThan(total);
    expect(total).toBe(fixture.bytes.length);
    expect(body.equals(fixture.bytes.subarray(start, end + 1))).toBe(true);
  }
  expect(response.headers()["content-length"]).toBe(String(body.length));
  const original = await page.request.get(fixture.path);
  expect(original.status()).toBe(200);
  expect(original.headers()["x-hito-stream-quality"]).toBe("original");
  expect((await original.body()).equals(fixture.bytes)).toBe(true);
  return {
    source: response.url(),
    status: response.status(),
    requestRange,
    contentRange: contentRange ?? null,
  };
}

export async function verifyUnsupportedConverted(
  page: Page,
  response: Response,
  fixture: { path: string },
  quality: "high" | "saver",
) {
  const url = new URL(response.url());
  expect(url.pathname).toBe(fixture.path);
  expect(url.searchParams.get("quality")).toBe(quality);
  expect(response.status()).toBe(200);
  expect(response.headers()["content-type"]).toMatch(/^audio\/aac(?:;|$)/);
  expect(response.headers()["x-hito-stream-quality"]).toBe(quality);
  expect(response.headers()["x-hito-stream-start-ms"]).toBe(
    url.searchParams.get("start_ms"),
  );
  const complete = await page.request.get(`${url.pathname}${url.search}`);
  expect(complete.status()).toBe(200);
  expect(complete.headers()["content-type"]).toMatch(/^audio\/aac(?:;|$)/);
  expect(complete.headers()["x-hito-stream-quality"]).toBe(quality);
  const body = await complete.body();
  expect(body.length).toBeGreaterThan(1024);
  expect(body[0]).toBe(0xff);
  expect(body[1] & 0xf6).toBe(0xf0);
  return {
    source: response.url(),
    browserBodyFailure: (await response.finished())?.message ?? null,
  };
}

export async function assertNoQualityDownloads(
  page: Page,
  recordings: string[],
) {
  for (const recording of recordings) {
    const response = await page.request.get(
      `/api/v1/recordings/${recording}/download`,
    );
    expect(response.status()).toBe(404);
    expect(await response.json()).toEqual({ error: "Resource not found" });
  }
}
