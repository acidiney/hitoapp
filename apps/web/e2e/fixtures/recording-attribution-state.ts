import { chooseMusicService } from "./music-service";
import { isDeepStrictEqual } from "node:util";
import { expect, type Locator, type Page } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import { reviewPluginInstallation } from "./plugin-installation-review";
import type { artistReviewReader } from "./artist-review-state";
import {
  preferencesAction,
  type seedPreferencesLibrary,
} from "./metadata-preferences-state";
import type { recordingAttributionProvider } from "./recording-attribution-provider";

type Reader = ReturnType<typeof artistReviewReader>;
type Provider = Awaited<ReturnType<typeof recordingAttributionProvider>>;

export async function connectRecordingProvider(page: Page, owned: Provider) {
  await page.goto("/settings?section=plugins");
  await page.getByText("Services for this server", { exact: true }).click();
  await page.getByLabel("Service address", { exact: true }).fill(owned.address);
  const offered = await preferencesAction<Schema<"PluginService">>(
    page,
    page.getByRole("button", { name: "Offer service", exact: true }),
    "POST",
    "/api/v1/plugins/services",
    201,
  );
  if (!offered.body)
    throw new Error("Owned recording service offer is missing");
  expect(
    isDeepStrictEqual(offered.sent, { base_url: owned.address }) &&
      offered.body.base_url === owned.address &&
      offered.body.name === owned.manifest.name,
  ).toBe(true);
  await page
    .getByRole("combobox", { name: "Service", exact: true })
    .selectOption(offered.body.id);
  await page
    .getByLabel("Bearer token (optional)", { exact: true })
    .fill(owned.token);
  const reviewed = await reviewPluginInstallation(
    page,
    offered.body,
    owned.manifest.name,
  );
  const installed = await preferencesAction<Schema<"PluginInstallation">>(
    page,
    page.getByRole("button", { name: "Accept and connect", exact: true }),
    "POST",
    "/api/v1/plugins",
    201,
  );
  if (!installed.body) throw new Error("Owned recording connection is missing");
  expect(
    installed.body.service_id === offered.body.id &&
      installed.body.credential_set &&
      isDeepStrictEqual(installed.body.manifest, owned.manifest) &&
      isDeepStrictEqual(installed.sent, {
        ...reviewed.sent,
        accept_manifest_digest: reviewed.body.digest,
      }),
  ).toBe(true);
  return installed.body;
}

export async function discoverRecording(
  page: Page,
  query: string,
  connection: Schema<"PluginInstallation">,
  expected: Schema<"ProviderRecording">,
) {
  await page.goto("/discover");
  await chooseMusicService(page, page, connection.id);
  await page.getByLabel("Search music", { exact: true }).fill(query);
  const searched = await preferencesAction<Schema<"SearchPage">>(
    page,
    page.getByRole("button", { name: "Search", exact: true }),
    "POST",
    `/api/v1/plugins/${connection.id}/search`,
    200,
  );
  expect(
    isDeepStrictEqual(searched.sent, {
      installation_revision: connection.revision,
      query,
      cursor: null,
      limit: 20,
    }) &&
      isDeepStrictEqual(searched.body, {
        items: [expected],
        next_cursor: null,
      }),
  ).toBe(true);
  const results = page.getByRole("list", {
    name: "Music search results",
    exact: true,
  });
  await expect(results.getByRole("listitem")).toHaveCount(1);
  const adopted = await preferencesAction<Schema<"Recording">>(
    page,
    results.getByRole("button", {
      name: `Add ${expected.title} to library`,
      exact: true,
    }),
    "POST",
    `/api/v1/plugins/${connection.id}/recordings`,
    200,
  );
  if (!adopted.body) throw new Error("Owned recording adoption is missing");
  expect(
    isDeepStrictEqual(adopted.sent, {
      installation_revision: connection.revision,
      provider_recording_id: expected.id,
    }) &&
      adopted.body.title === expected.title &&
      adopted.body.artist === expected.artist &&
      !adopted.body.available,
  ).toBe(true);
  return adopted.body;
}

export async function openSavedRecording(
  page: Page,
  recording: Schema<"Recording">,
  title: string,
) {
  const link = page.getByRole("link", {
    name: `View ${title} in library`,
    exact: true,
  });
  await expect(link).toHaveAttribute("href", `/recordings/${recording.id}`);
  await link.click();
  await expect(page).toHaveURL(new RegExp(`/recordings/${recording.id}$`));
}

export async function verifyCredits(
  region: Locator,
  credit: Schema<"RecordingAttribution">,
) {
  await expect(region).toBeVisible();
  const creators = region.getByRole("list", { name: "Creators", exact: true });
  expect(
    isDeepStrictEqual(
      await creators.getByRole("listitem").allTextContents(),
      credit.creators,
    ),
  ).toBe(true);
  const link = region.getByRole("link", {
    name: credit.provider_label,
    exact: true,
  });
  expect(
    (await link.getAttribute("href")) === credit.content_url &&
      (await link.getAttribute("target")) === "_blank" &&
      (await link.getAttribute("rel"))?.split(" ").sort().join(",") ===
        "noopener,noreferrer" &&
      (await link.getAttribute("referrerpolicy")) === "no-referrer",
  ).toBe(true);
  await expect(region.locator("img,script,iframe,style,link")).toHaveCount(0);
  expect(
    await region
      .locator(".recording-credits")
      .evaluateAll((lists) =>
        lists.every(
          (list) =>
            list.scrollWidth <= list.clientWidth + 1 &&
            [...list.querySelectorAll("dd,li")].every(
              (element) => element.scrollWidth <= element.clientWidth + 1,
            ),
        ),
      ),
  ).toBe(true);
}

export async function localRecordingState(
  page: Page,
  read: Reader,
  saved: Awaited<ReturnType<typeof seedPreferencesLibrary>>,
) {
  const response = await page.request.post("/api/v1/recordings/progress", {
    headers: { "x-hito-request": "1", Origin: new URL(page.url()).origin },
    data: { recording_ids: saved.ids } satisfies Schema<"SavedPlaybackRequest">,
    timeout: 10_000,
  });
  let progress: Schema<"RecordingProgress">[];
  try {
    const body = await response.body();
    if (response.status() !== 200 || body.length > 16_384)
      throw new Error("Owned recording progress read failed");
    progress = JSON.parse(body.toString()) as Schema<"RecordingProgress">[];
  } finally {
    await response.dispose();
  }
  const catalog = await read<Schema<"RecordingPage">>("/api/v1/recordings");
  return {
    catalog: catalog.items.filter((recording) =>
      saved.ids.includes(recording.id),
    ),
    playlist: await read<Schema<"PlaylistDetail">>(
      `/api/v1/playlists/${saved.playlist}`,
    ),
    metadata: await Promise.all(
      saved.ids.map((id) =>
        read<Schema<"RecordingMetadata">>(`/api/v1/recordings/${id}/metadata`),
      ),
    ),
    sources: await Promise.all(
      saved.ids.map((id) =>
        read<Schema<"Source">[]>(`/api/v1/recordings/${id}/sources`),
      ),
    ),
    activity: await read<Schema<"Activity">>("/api/v1/activity"),
    events: await read<Schema<"SyncPage">>("/api/v1/sync?cursor=0"),
    progress,
  };
}

export async function retainedProviderState(read: Reader, recording: string) {
  const metadata = await read<Schema<"RecordingMetadata">>(
    `/api/v1/recordings/${recording}/metadata`,
  );
  return {
    title: metadata.title,
    artist: metadata.artist,
    choice: await read<Schema<"SourceChoice">>(
      `/api/v1/recordings/${recording}/source-choice`,
    ),
    sources: await read<Schema<"Source">[]>(
      `/api/v1/recordings/${recording}/sources`,
    ),
    settings: await read<Schema<"PluginSettings">>("/api/v1/plugins"),
    preferences: await read<Schema<"MetadataSettings">>(
      "/api/v1/metadata/settings",
    ),
  };
}
