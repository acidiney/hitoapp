import type { APIResponse, Page, Response } from "@playwright/test";

export function podcastHistoryFeed() {
  const raw =
    process.env.HITO_PODCAST_HISTORY_FEED ??
    "https://feeds.simplecast.com/ihq5xr73";
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

export async function podcastHistoryJson<T>(
  response: Response | APIResponse,
  status = 200,
): Promise<T> {
  const body = await response.body();
  if (body.length > 524_288)
    throw new Error("A history verification response exceeded its bound");
  if (response.status() !== status)
    throw new Error("A history verification response failed");
  return JSON.parse(body.toString()) as T;
}

export function podcastHistoryReader(page: Page) {
  let requests = 0;
  return {
    count: () => requests,
    async get<T>(path: "/api/v1/setup" | "/api/v1/podcasts"): Promise<T> {
      if (++requests > 70)
        throw new Error("The history verification read budget was exceeded");
      const response = await page.request.get(path, { timeout: 10_000 });
      try {
        return await podcastHistoryJson<T>(response);
      } finally {
        await response.dispose();
      }
    },
  };
}
