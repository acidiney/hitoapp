import { createHash, randomUUID } from "node:crypto";
import { expect, type Locator, type Page } from "@playwright/test";
import type { components } from "../../src/generated";
import type { ArtworkProviderFixture } from "./collection-artwork-provider";
import { reviewPluginInstallation } from "./plugin-installation-review";

type Schema = components["schemas"];
export const hash = (bytes: Buffer) =>
  createHash("sha256").update(bytes).digest("hex");

export async function json<T>(page: Page, path: string): Promise<T> {
  const response = await page.request.get(path, {
    timeout: 10_000,
    maxRedirects: 0,
  });
  try {
    expect(response.status()).toBe(200);
    const bytes = await response.body();
    expect(bytes.length).toBeLessThanOrEqual(524_288);
    return JSON.parse(bytes.toString()) as T;
  } finally {
    await response.dispose();
  }
}

export async function downloadAbsence(
  page: Page,
  account: string,
  recordings: string[],
  playlists: string[],
) {
  expect((await json<Schema["User"]>(page, "/api/v1/me")).id).toBe(account);
  expect(new Set(recordings).size).toBe(recordings.length);
  expect(new Set(playlists).size).toBe(playlists.length);
  const paths = [
    ...recordings.map((id) => `/api/v1/recordings/${id}/download`),
    ...playlists.map((id) => `/api/v1/playlists/${id}/download`),
  ];
  for (const path of paths) {
    const response = await page.request.get(path, {
      timeout: 10_000,
      maxRedirects: 0,
      maxRetries: 0,
      headers: { "x-hito-request": "1", "x-hito-account": account },
    });
    try {
      expect(response.status()).toBe(404);
      expect(response.headers()["cache-control"]).toBe("private, no-store");
      const bytes = await response.body();
      expect(bytes.length).toBeLessThanOrEqual(4096);
      expect(JSON.parse(bytes.toString()) as Schema["ErrorResponse"]).toEqual({
        error: "Resource not found",
      });
    } finally {
      await response.dispose();
    }
  }
  return {
    recordings: recordings.length,
    playlists: playlists.length,
    reads: paths.length,
    status: 404,
  };
}

export function mutationHeaders(page: Page) {
  return { "x-hito-request": "1", origin: new URL(page.url()).origin };
}

export async function action<T>(
  page: Page,
  path: string,
  status: number,
  control: Locator,
  method = "POST",
): Promise<T> {
  const [response] = await Promise.all([
    page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === path &&
        response.request().method() === method,
    ),
    control.click(),
  ]);
  expect(response.status()).toBe(status);
  if (status === 204) return null as T;
  const bytes = await response.body();
  expect(bytes.length).toBeLessThanOrEqual(32_768);
  return bytes.length ? (JSON.parse(bytes.toString()) as T) : (null as T);
}

export async function installProvider(
  page: Page,
  fixture: ArtworkProviderFixture,
  existing?: Schema["PluginService"],
) {
  await page.goto("/settings?section=plugins");
  let service = existing;
  if (!service) {
    await page.getByText("Services for this server", { exact: true }).click();
    await page
      .getByLabel("Service address", { exact: true })
      .fill(fixture.address);
    service = await action<Schema["PluginService"]>(
      page,
      "/api/v1/plugins/services",
      201,
      page.getByRole("button", { name: "Offer service", exact: true }),
    );
  }
  await page
    .getByRole("combobox", { name: "Service", exact: true })
    .selectOption(service.id);
  await page
    .getByLabel("Bearer token (optional)", { exact: true })
    .fill(fixture.token);
  const review = await reviewPluginInstallation(
    page,
    service,
    fixture.manifest.name,
  );
  expect(review.body.manifest.capabilities).toEqual(
    fixture.manifest.capabilities,
  );
  const installation = await action<Schema["PluginInstallation"]>(
    page,
    "/api/v1/plugins",
    201,
    page.getByRole("button", { name: "Accept and connect", exact: true }),
  );
  expect(installation.manifest.capabilities).toEqual(
    fixture.manifest.capabilities,
  );
  return { service, installation };
}

export function connectionCard(page: Page, fixture: ArtworkProviderFixture) {
  return page.locator(".plugin-connection").filter({
    has: page.getByRole("heading", {
      name: fixture.manifest.name,
      exact: true,
    }),
  });
}

export async function approveArtwork(
  page: Page,
  fixture: ArtworkProviderFixture,
  installation: Schema["PluginInstallation"],
) {
  fixture.enableArtwork();
  await page.goto("/settings?section=plugins");
  const card = connectionCard(page, fixture);
  const path = `/api/v1/plugins/${installation.id}/refresh`;
  const review = await action<Schema["PluginRefresh"]>(
    page,
    path,
    200,
    card.getByRole("button", { name: "Check connection", exact: true }),
  );
  expect(review.kind).toBe("review_required");
  await expect(
    card.getByRole("heading", { name: "Review plugin changes", exact: true }),
  ).toBeVisible();
  return { card, path };
}

export async function removeProvider(
  page: Page,
  fixture: ArtworkProviderFixture,
  installation: Schema["PluginInstallation"],
) {
  await page.goto("/settings?section=plugins");
  const card = connectionCard(page, fixture);
  await card.getByRole("button", { name: "Remove", exact: true }).click();
  await action(
    page,
    `/api/v1/plugins/${installation.id}`,
    204,
    card.getByRole("button", { name: "Remove plugin", exact: true }),
    "DELETE",
  );
}

export async function importCopy(
  page: Page,
  fixture: ArtworkProviderFixture,
  installation: Schema["PluginInstallation"],
  selection = fixture.selections[0],
) {
  await page.goto("/playlists");
  const form = page.getByRole("region", { name: "Import a playlist" });
  await form
    .getByRole("combobox", { name: "Playlist service" })
    .selectOption(installation.id);
  await form
    .getByRole("textbox", { name: "Playlist link or identifier" })
    .fill(selection.reference);
  const receipt = await action<Schema["PlaylistImportReceipt"]>(
    page,
    `/api/v1/plugins/${installation.id}/playlists/import`,
    200,
    form.getByRole("button", { name: "Import a copy", exact: true }),
  );
  await expect(page).toHaveURL(
    new RegExp(`/playlists/${receipt.playlist_id}$`),
  );
  const detail = await json<Schema["PlaylistDetail"]>(
    page,
    `/api/v1/playlists/${receipt.playlist_id}`,
  );
  expect(detail.origin?.provider_playlist_id).toBe(selection.detail.id);
  expect(detail.origin?.installation_id).toBe(installation.id);
  expect(detail.origin?.snapshot).toBe(selection.detail.snapshot);
  expect(detail.entries).toHaveLength(2);
  expect(detail.entries.map((entry) => entry.origin?.occurrence_id)).toEqual([
    "first",
    "duplicate",
  ]);
  expect(new Set(detail.entries.map((entry) => entry.recording_id)).size).toBe(
    1,
  );
  expect(new Set(detail.entries.map((entry) => entry.entry_id)).size).toBe(2);
  return detail;
}

export async function preservePersonalEdits(
  page: Page,
  copies: string[],
  recording: string,
  release: string,
  prefix: string,
) {
  const titles = {
    track: `${prefix} personal title`,
    artist: `${prefix} personal artist`,
    release: `${prefix} personal album`,
    copies: copies.map((_, index) => `${prefix} private copy ${index + 1}`),
  };
  const actions: Schema["Action"][] = [
    {
      kind: "recording_title_set",
      recording_id: recording,
      title: titles.track,
    },
    {
      kind: "recording_artist_set",
      recording_id: recording,
      artist: titles.artist,
    },
    { kind: "release_title_set", release_id: release, title: titles.release },
    { kind: "like_set", recording_id: recording, liked: true },
    {
      kind: "progress_set",
      recording_id: recording,
      position_ms: 4200,
      speed: 1.25,
    },
    ...copies.map((playlist_id, index) => ({
      kind: "playlist_rename" as const,
      playlist_id,
      name: titles.copies[index],
    })),
  ];
  const response = await page.request.post("/api/v1/sync", {
    headers: mutationHeaders(page),
    timeout: 10_000,
    data: {
      device_id: "owned-artwork-browser",
      operations: actions.map((action) => ({ id: randomUUID(), action })),
    } satisfies Schema["SyncBatch"],
  });
  expect(response.status()).toBe(200);
  await response.dispose();
  return titles;
}

export async function decoded(image: Locator, path: string) {
  await expect(image).toHaveAttribute("src", path);
  await expect
    .poll(() =>
      image.evaluate(
        (element: HTMLImageElement) =>
          element.complete && element.naturalWidth > 0,
      ),
    )
    .toBe(true);
}

export async function retained(page: Page, path: string, expected: Buffer) {
  const response = await page.request.get(path, {
    timeout: 10_000,
    maxRedirects: 0,
  });
  try {
    expect(response.status()).toBe(200);
    expect(response.headers()["cache-control"]).toBe("private, no-store");
    expect(response.headers()["content-type"]).toBe("image/png");
    const body = await response.body();
    expect(body).toEqual(expected);
    return hash(body);
  } finally {
    await response.dispose();
  }
}
