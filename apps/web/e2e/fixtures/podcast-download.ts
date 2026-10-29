import { expect, type Page } from "@playwright/test";
import type { components } from "../../src/generated";

export type Schema = components["schemas"];
export type PodcastFixtureStatus = {
  kind: string;
  feed_url: string;
  feed_requests: number;
  enclosure_requests: number;
  unexpected_requests: number;
  upstream_closed: boolean;
};

export async function read<T>(page: Page, path: string): Promise<T> {
  const response = await page.request.get(path);
  expect(response.status()).toBe(200);
  return response.json();
}

export function fixtureStatus(page: Page) {
  return read<PodcastFixtureStatus>(page, "/__podcast_fixture");
}

export async function stopUpstream(page: Page, origin: string) {
  const response = await page.request.post("/__podcast_fixture/stop", {
    headers: { origin, "x-hito-request": "1" },
    timeout: 10_000,
  });
  expect(response.status()).toBe(200);
  const status: PodcastFixtureStatus = await response.json();
  expect(status.upstream_closed).toBe(true);
  return status;
}

export async function progress(page: Page, recording: string, origin: string) {
  const response = await page.request.post("/api/v1/recordings/progress", {
    headers: { origin, "x-hito-request": "1" },
    data: {
      recording_ids: [recording],
    } satisfies Schema["SavedPlaybackRequest"],
  });
  expect(response.status()).toBe(200);
  const items: Schema["RecordingProgress"][] = await response.json();
  expect(items).toHaveLength(1);
  return items[0];
}

async function sourceAudio(page: Page, source: string) {
  const response = await page.request.get(`/api/v1/sources/${source}/audio`);
  expect(response.status()).toBe(200);
  expect(response.headers()["content-type"]).toBe("audio/wav");
  return response.body();
}

export async function prepareOwnedOriginal(page: Page) {
  await page
    .getByRole("link", { name: "Account and settings", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Add music folder", exact: true })
    .click();
  const picker = page.getByRole("dialog", {
    name: "Add a music folder",
    exact: true,
  });
  await picker
    .getByRole("button", { name: "Open Owned original folder", exact: true })
    .click();
  await picker
    .getByRole("button", { name: "Use this folder", exact: true })
    .click();
  await expect(picker).not.toBeVisible();
  await expect(
    page.locator(".music-folder-row").filter({ hasText: "1 file read" }),
  ).toHaveCount(1, { timeout: 20_000 });
  const recordings = await read<Schema["RecordingPage"]>(
    page,
    "/api/v1/recordings",
  );
  expect(recordings.items).toHaveLength(1);
  const recording = recordings.items[0];
  const sourcePath = `/api/v1/recordings/${recording.id}/sources`;
  const sources = await read<Schema["Source"][]>(page, sourcePath);
  expect(sources).toHaveLength(1);
  expect(sources[0].provider === "local" && sources[0].ready).toBe(true);
  const audio = await sourceAudio(page, sources[0].id);
  expect(audio.length).toBe(720_044);
  expect(audio.subarray(0, 4).toString()).toBe("RIFF");
  return {
    recording,
    audio,
    async assertRetained(reader: Page) {
      const current = await read<Schema["RecordingPage"]>(
        reader,
        `/api/v1/recordings?library=${recording.library_id}`,
      );
      const currentSources = await read<Schema["Source"][]>(reader, sourcePath);
      expect(JSON.stringify(current) === JSON.stringify(recordings)).toBe(true);
      expect(JSON.stringify(currentSources) === JSON.stringify(sources)).toBe(
        true,
      );
      expect((await sourceAudio(reader, sources[0].id)).equals(audio)).toBe(
        true,
      );
    },
  };
}
