import { randomUUID } from "node:crypto";
import {
  expect,
  type Locator,
  type Page,
  type TestInfo,
} from "@playwright/test";
import type { components } from "../../src/generated";
import { paceAuthAttempt } from "./auth-pacing";

type Schema = components["schemas"];
type SavedState = {
  playlists: Schema["Playlist"][];
  copies: Schema["PlaylistDetail"][];
  catalog: Schema["RecordingPage"];
  metadata: Schema["RecordingMetadata"];
  activity: Schema["Activity"];
  feed: Schema["SyncPage"];
  sources: Schema["Source"][];
};

export async function signIn(page: Page, username?: string, password?: string) {
  const firstRun = username === undefined && password === undefined;
  const setupResponse = await page.request.get("/api/v1/setup");
  expect(setupResponse.status()).toBe(200);
  const setup: Schema["SetupStatus"] = await setupResponse.json();
  expect(setup.needs_setup).toBe(firstRun);
  await paceAuthAttempt();
  await page.goto("/playlists");
  await page.getByLabel("Username", { exact: true }).fill(username ?? "admin");
  await page
    .getByLabel("Password", { exact: true })
    .fill(password ?? "docker browser test password");
  await page
    .getByRole("button", {
      name: firstRun ? "Create administrator" : "Sign in",
      exact: true,
    })
    .click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
}

export async function get<T>(page: Page, path: string): Promise<T> {
  const response = await page.request.get(path);
  expect(response.status()).toBe(200);
  return response.json();
}

export async function savedState(
  page: Page,
  copies: string[],
  recording: string,
): Promise<SavedState> {
  return {
    playlists: await get(page, "/api/v1/playlists"),
    copies: await Promise.all(
      copies.map((id) =>
        get<Schema["PlaylistDetail"]>(page, `/api/v1/playlists/${id}`),
      ),
    ),
    catalog: await get(page, "/api/v1/recordings"),
    metadata: await get(page, `/api/v1/recordings/${recording}/metadata`),
    activity: await get(page, "/api/v1/activity"),
    feed: await get(page, "/api/v1/sync?cursor=0"),
    sources: await get(page, `/api/v1/recordings/${recording}/sources`),
  };
}

export async function savePersonalState(
  page: Page,
  copies: string[],
  recordingId: string,
  title: string,
  artist: string,
  initial: Schema["Playlist"][],
  headers: Record<string, string>,
): Promise<SavedState> {
  const edited = await page.request.post("/api/v1/sync", {
    headers,
    data: {
      device_id: "provider-artwork-e2e",
      operations: [
        {
          id: randomUUID(),
          action: {
            kind: "recording_title_set",
            recording_id: recordingId,
            title,
          },
        },
        {
          id: randomUUID(),
          action: {
            kind: "recording_artist_set",
            recording_id: recordingId,
            artist,
          },
        },
        {
          id: randomUUID(),
          action: {
            kind: "like_set",
            recording_id: recordingId,
            liked: true,
          },
        },
        {
          id: randomUUID(),
          action: {
            kind: "progress_set",
            recording_id: recordingId,
            position_ms: 4200,
            speed: 1.25,
          },
        },
      ],
    } satisfies Schema["SyncBatch"],
  });
  expect(edited.status()).toBe(200);
  const before = await savedState(page, copies, recordingId);
  expect(before.playlists).toHaveLength(initial.length + 2);
  expect(
    new Set(
      before.copies.flatMap((copy) =>
        copy.entries.map((entry) => entry.entry_id),
      ),
    ).size,
  ).toBe(4);
  expect(before.metadata.title.personal).toBe(title);
  expect(before.metadata.artist.personal).toBe(artist);
  expect(
    before.catalog.items.find((item) => item.id === recordingId),
  ).toMatchObject({ title, artist, liked: true, available: false });
  expect(
    before.activity.history.find((item) => item.id === recordingId),
  ).toMatchObject({ liked: true, position_ms: 4200, speed: 1.25 });
  expect(before.sources).toEqual([]);
  return before;
}

export async function layouts(
  page: Page,
  info: TestInfo,
  stage: string,
  controls: Locator[],
) {
  const measured: {
    viewport: number;
    label: string;
    width: number;
    height: number;
  }[] = [];
  for (const viewport of [
    { width: 1440, height: 1000 },
    { width: 390, height: 844 },
  ]) {
    await page.setViewportSize(viewport);
    for (const control of controls) {
      await control.evaluate((element) =>
        element.scrollIntoView({ block: "center", inline: "center" }),
      );
      await expect(control).toBeVisible();
      const bounds = await control.boundingBox();
      const dock = await page
        .getByRole("contentinfo", { name: "Audio player" })
        .boundingBox();
      if (!bounds || !dock)
        throw new Error("The control and player need visible bounds");
      expect(bounds.x).toBeGreaterThanOrEqual(0);
      expect(bounds.x + bounds.width).toBeLessThanOrEqual(viewport.width);
      expect(bounds.y + bounds.height).toBeLessThanOrEqual(dock.y);
      if (
        await control.evaluate((element) =>
          element.matches("input, select, .track-actions .icon-button"),
        )
      ) {
        expect(bounds.width).toBeGreaterThanOrEqual(44);
        expect(bounds.height).toBeGreaterThanOrEqual(44);
      }
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
      measured.push({
        viewport: viewport.width,
        label:
          (await control.getAttribute("aria-label")) ??
          (await control.innerText()),
        width: bounds.width,
        height: bounds.height,
      });
    }
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      ),
    ).toBe(false);
    await page.screenshot({
      path: info.outputPath(`${stage}-${viewport.width}.png`),
    });
  }
  await page.setViewportSize({ width: 1440, height: 1000 });
  return measured;
}

export async function loadedCopies(
  page: Page,
  copies: string[],
  recording: string,
  title: string,
  baseURL: string,
) {
  for (const copy of copies) {
    await page.goto(`/playlists/${copy}`);
    const rows = page
      .getByRole("table", { name: "Playlist entries" })
      .locator("tbody")
      .getByRole("row");
    await expect(rows).toHaveCount(2);
    await expect(rows.locator(".track-title")).toHaveText([title, title]);
    await expect(
      page.getByRole("button", { name: "Play playlist", exact: true }),
    ).toBeEnabled();
    for (const row of await rows.all()) {
      await expect(
        row.getByRole("button", { name: `Play ${title}`, exact: true }),
      ).toBeEnabled();
      const image = row.locator(".artwork img");
      await expect(image).toHaveAttribute(
        "src",
        `/api/v1/recordings/${recording}/artwork`,
      );
      await expect
        .poll(() =>
          image.evaluate(
            (element: HTMLImageElement) =>
              element.complete && element.naturalWidth > 0,
          ),
        )
        .toBe(true);
    }
    for (const image of await page.locator("img").all()) {
      const url = await image.evaluate(
        (element: HTMLImageElement) => element.currentSrc || element.src,
      );
      expect(new URL(url).origin).toBe(new URL(baseURL).origin);
    }
    await expect(page.locator("audio")).not.toHaveAttribute("src");
  }
}
