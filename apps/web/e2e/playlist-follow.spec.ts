import { expect, test, type Locator, type Page } from "@playwright/test";
import type { components } from "../src/generated";
import { playlistFollowFixture } from "./fixtures/playlist-follow";
import {
  connectLiveProvider,
  setUpSpotifyYouTubeLiveAccount as setUpDisposableFollowAccount,
} from "./fixtures/spotify-youtube-live";

type Schema = components["schemas"];
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

test.use({ trace: "off", screenshot: "off", video: "off" });

test.describe("isolated provider playlist Follow", () => {
  test.skip(
    process.env.HITO_PLAYLIST_FOLLOW_E2E !== "1",
    "Requires explicit disposable Follow server opt-in",
  );

  test("a daily Follow refreshes witnessed occurrences while its editable copy and private edits stay intact", async ({
    page,
    baseURL,
  }) => {
    let phase = 0;
    let failed = false;
    let pageErrors = 0;
    let externalRequests = 0;
    let audioRequests = 0;
    let imports = 0;
    let follows = 0;
    let refreshes = 0;
    let privateEdits = 0;
    let offers = 0;
    const origin = baseURL ? new URL(baseURL).origin : null;
    const fixture = await playlistFollowFixture();
    page.on("pageerror", () => pageErrors++);
    page.on("request", (request) => {
      const url = new URL(request.url());
      if (!["http:", "https:"].includes(url.protocol)) return;
      if (url.origin !== origin) externalRequests++;
      if (request.resourceType() === "media" || /\/audio$/.test(url.pathname))
        audioRequests++;
      if (request.method() !== "POST") return;
      if (/\/plugins\/[^/]+\/playlists\/import$/.test(url.pathname)) imports++;
      if (/\/plugins\/[^/]+\/playlists\/follow$/.test(url.pathname)) follows++;
      if (/\/playlists\/[^/]+\/follow\/refresh$/.test(url.pathname))
        refreshes++;
      if (url.pathname === "/api/v1/sync") privateEdits++;
    });
    try {
      phase = 1;
      if (!origin)
        throw new Error("The disposable Follow server must be configured");
      await setUpDisposableFollowAccount(page);
      const connection = await connectLiveProvider(
        page,
        fixture.address,
        fixture.manifest.name,
        () => offers++,
      );
      expect(connection.manifest.capabilities.join(",") === "playlists").toBe(
        true,
      );
      expect(!connection.credential_set).toBe(true);

      phase = 2;
      await openPlaylists(page);
      const form = page.getByRole("region", {
        name: "Import a playlist",
        exact: true,
      });
      await form
        .getByRole("radio", { name: "Import a copy", exact: true })
        .check();
      await form
        .getByRole("combobox", { name: "Playlist service" })
        .selectOption(connection.id);
      await form
        .getByRole("textbox", { name: "Playlist link or identifier" })
        .fill(fixture.reference);
      const copyResponse = postResponse(
        page,
        `/api/v1/plugins/${connection.id}/playlists/import`,
      );
      await form
        .getByRole("button", { name: "Import a copy", exact: true })
        .click();
      const copiedResponse = await copyResponse;
      expect(copiedResponse.status()).toBe(200);
      const copyReceipt: Schema["PlaylistImportReceipt"] =
        await copiedResponse.json();
      const copyRequest: Schema["ImportProviderPlaylist"] = copiedResponse
        .request()
        .postDataJSON();
      expect(
        uuid.test(copyReceipt.playlist_id) &&
          copyReceipt.request_id === copyRequest.request_id,
      ).toBe(true);
      expect(copyRequest.playlist_reference === fixture.reference).toBe(true);
      await expect
        .poll(
          () =>
            new URL(page.url()).pathname ===
            `/playlists/${copyReceipt.playlist_id}`,
        )
        .toBe(true);
      const copyPath = `/api/v1/playlists/${copyReceipt.playlist_id}`;
      const copied = await read<Schema["PlaylistDetail"]>(page, copyPath);
      expect(
        copied.playlist.kind === "local" && copied.entries.length === 5,
      ).toBe(true);
      const table = page.getByRole("table", {
        name: "Playlist entries",
        exact: true,
      });
      await expect(table.locator("tbody").getByRole("row")).toHaveCount(5);
      await expect(
        table.getByRole("button", { name: /^Remove .* from playlist$/ }),
      ).toHaveCount(5);
      const recording = copied.entries[0].recording_id;
      if (!recording || !uuid.test(recording))
        throw new Error("The owned recording identity is missing");

      phase = 3;
      await table
        .locator("tbody")
        .getByRole("row")
        .first()
        .getByRole("link", {
          name: `Details for ${fixture.recordings.a.title}`,
          exact: true,
        })
        .click();
      const editor = page.getByRole("region", {
        name: "Your track details",
        exact: true,
      });
      for (const [field, value] of [
        ["title", "My retained Follow title"],
        ["artist", "My retained Follow artist"],
      ] as const) {
        await editor
          .getByRole("textbox", { name: `Track ${field}`, exact: true })
          .fill(value);
        const changed = postResponse(page, "/api/v1/sync");
        await editor
          .getByRole("button", { name: `Save personal ${field}`, exact: true })
          .click();
        expect((await changed).status()).toBe(200);
        await expect(
          editor.getByRole("button", {
            name: `Use source ${field}`,
            exact: true,
          }),
        ).toBeVisible();
      }
      const metadataPath = `/api/v1/recordings/${recording}/metadata`;
      const metadataBefore = await read<Schema["RecordingMetadata"]>(
        page,
        metadataPath,
      );
      expect(metadataBefore.title.personal === "My retained Follow title").toBe(
        true,
      );
      expect(
        metadataBefore.artist.personal === "My retained Follow artist",
      ).toBe(true);
      const copyBefore = await read<Schema["PlaylistDetail"]>(page, copyPath);

      phase = 4;
      await openPlaylists(page);
      await form
        .getByRole("radio", { name: "Follow playlist", exact: true })
        .check();
      await form
        .getByRole("combobox", { name: "Playlist service" })
        .selectOption(connection.id);
      await form
        .getByRole("combobox", { name: "Refresh interval", exact: true })
        .selectOption({ label: "Every day" });
      await form
        .getByRole("textbox", { name: "Playlist link or identifier" })
        .fill(fixture.reference);
      const followedResponse = postResponse(
        page,
        `/api/v1/plugins/${connection.id}/playlists/follow`,
      );
      await form
        .getByRole("button", { name: "Follow playlist", exact: true })
        .click();
      const created = await followedResponse;
      expect(created.status()).toBe(200);
      const followReceipt: Schema["PlaylistFollowReceipt"] =
        await created.json();
      const followRequest: Schema["FollowProviderPlaylist"] = created
        .request()
        .postDataJSON();
      expect(
        uuid.test(followReceipt.playlist_id) &&
          followReceipt.request_id === followRequest.request_id,
      ).toBe(true);
      expect(
        followRequest.interval === "daily" &&
          followRequest.playlist_reference === fixture.reference,
      ).toBe(true);
      expect(followReceipt.playlist_id !== copyReceipt.playlist_id).toBe(true);
      await expect
        .poll(
          () =>
            new URL(page.url()).pathname ===
            `/playlists/${followReceipt.playlist_id}`,
        )
        .toBe(true);
      const detailPath = `/api/v1/playlists/${followReceipt.playlist_id}`;
      const statusPath = `${detailPath}/follow`;
      const before = await read<Schema["PlaylistDetail"]>(page, detailPath);
      expect(before.playlist.kind === "followed").toBe(true);
      expect(before.entries.length).toBe(5);
      expect(
        before.entries.every(
          (entry) => uuid.test(entry.entry_id) && !entry.available,
        ),
      ).toBe(true);
      expect(new Set(before.entries.map((entry) => entry.entry_id)).size).toBe(
        5,
      );
      expect(
        before.entries[0].recording_id === recording &&
          before.entries[4].recording_id === recording,
      ).toBe(true);
      expect(
        before.entries[2].origin?.kind === "unknown" &&
          before.entries[2].recording_id === null,
      ).toBe(true);
      expect(
        before.entries[3].origin?.kind === "unavailable" &&
          before.entries[3].recording_id === null,
      ).toBe(true);
      samePrivateState(
        before.entries.map((entry) => entry.origin?.occurrence_id),
        ["first-a", "removed-b", "unknown", "withdrawn", "duplicate-a"],
      );
      const updates = page.getByRole("region", {
        name: "Playlist updates",
        exact: true,
      });
      await expect(updates.getByText("Ready", { exact: true })).toBeVisible();
      await expect(
        updates.getByText("Every day", { exact: true }),
      ).toBeVisible();
      await expect(page.getByText(/Read-only playlist/)).toBeVisible();
      await expect(page.getByText(/Editable copy/)).toHaveCount(0);
      await expect(
        table.getByRole("button", { name: /^Remove .* from playlist$/ }),
      ).toHaveCount(0);
      await expect(
        page.getByRole("button", { name: "Play playlist", exact: true }),
      ).toBeDisabled();
      const ready = await read<Schema["PlaylistFollow"]>(page, statusPath);
      expect(ready.status === "ready" && ready.interval === "daily").toBe(true);
      expect(ready.next_refresh_at - ready.last_refreshed_at).toBe(86400);
      const feedBefore = await read<Schema["SyncPage"]>(
        page,
        "/api/v1/sync?cursor=0",
      );
      expect(feedBefore.has_more).toBe(false);

      phase = 5;
      const gate = fixture.advanceAndHold();
      const refresh = updates.getByRole("button", {
        name: "Refresh now",
        exact: true,
      });
      const refreshResponse = postResponse(page, `${statusPath}/refresh`);
      await refresh.click();
      const queued = await refreshResponse;
      expect(queued.status()).toBe(202);
      const request: Schema["RefreshFollowedPlaylist"] = queued
        .request()
        .postDataJSON();
      const receipt: Schema["PlaylistFollowRefresh"] = await queued.json();
      expect(
        uuid.test(receipt.job_id) &&
          receipt.request_id === request.request_id &&
          receipt.playlist_id === before.playlist.id,
      ).toBe(true);
      await expect.poll(() => gate.entered, { timeout: 10000 }).toBe(true);
      await expect(updates.getByText(/^Updating/)).toBeVisible();
      await expect(refresh).toBeDisabled();
      await expect(table.locator("tbody").getByRole("row")).toHaveCount(5);
      await retainedTitles(
        table,
        before.entries.map((entry) => entry.title),
      );
      samePrivateState(
        await read<Schema["PlaylistDetail"]>(page, detailPath),
        before,
      );
      const running = await read<Schema["PlaylistFollow"]>(page, statusPath);
      expect(
        running.status === "running" && running.snapshot === ready.snapshot,
      ).toBe(true);
      samePrivateState(
        await read<Schema["PlaylistDetail"]>(page, copyPath),
        copyBefore,
      );

      phase = 6;
      gate.release();
      await expect
        .poll(
          async () =>
            (await read<Schema["PlaylistFollow"]>(page, statusPath)).status ===
            "ready",
          { timeout: 10000 },
        )
        .toBe(true);
      await expect(updates.getByText("Ready", { exact: true })).toBeVisible();
      await expect(refresh).toBeEnabled();
      await expect(table.locator("tbody").getByRole("row")).toHaveCount(4);
      const after = await read<Schema["PlaylistDetail"]>(page, detailPath);
      expect(
        after.playlist.id === before.playlist.id &&
          after.playlist.kind === "followed",
      ).toBe(true);
      expect(after.entries.every((entry) => uuid.test(entry.entry_id))).toBe(
        true,
      );
      samePrivateState(
        after.entries.map((entry) => [
          entry.position,
          entry.origin?.occurrence_id,
        ]),
        [
          [0, "duplicate-a"],
          [1, "added-c"],
          [2, "first-a"],
          [3, "withdrawn"],
        ],
      );
      expect(
        after.origin?.provider === fixture.manifest.id &&
          after.origin.installation_id === connection.id,
      ).toBe(true);
      expect(
        after.origin?.provider_playlist_id ===
          before.origin?.provider_playlist_id &&
          after.origin?.snapshot === fixture.updated.detail.snapshot,
      ).toBe(true);
      expect(after.entries[0].entry_id === before.entries[4].entry_id).toBe(
        true,
      );
      expect(after.entries[2].entry_id === before.entries[0].entry_id).toBe(
        true,
      );
      expect(after.entries[3].entry_id === before.entries[3].entry_id).toBe(
        true,
      );
      expect(
        after.entries[0].recording_id === recording &&
          after.entries[2].recording_id === recording,
      ).toBe(true);
      expect(
        !before.entries.some(
          (entry) => entry.entry_id === after.entries[1].entry_id,
        ),
      ).toBe(true);
      expect(
        !after.entries.some(
          (entry) =>
            entry.entry_id === before.entries[1].entry_id ||
            entry.entry_id === before.entries[2].entry_id,
        ),
      ).toBe(true);
      expect(after.entries.every((entry) => !entry.available)).toBe(true);
      await retainedTitles(
        table,
        after.entries.map((entry) => entry.title),
      );
      samePrivateState(
        await read<Schema["PlaylistDetail"]>(page, copyPath),
        copyBefore,
      );
      samePrivateState(
        await read<Schema["RecordingMetadata"]>(page, metadataPath),
        metadataBefore,
      );
      const feed = await read<Schema["SyncPage"]>(
        page,
        "/api/v1/sync?cursor=0",
      );
      expect(
        !feed.has_more &&
          feed.operations.length === feedBefore.operations.length + 1,
      ).toBe(true);
      samePrivateState(feed.operations.slice(0, -1), feedBefore.operations);
      const event = feed.operations.at(-1);
      expect(
        event &&
          "publication" in event &&
          event.publication.kind === "playlist_follow_refreshed",
      ).toBe(true);
      const recordingIds = new Set(
        [...copyBefore.entries, ...after.entries].flatMap((entry) =>
          entry.recording_id ? [entry.recording_id] : [],
        ),
      );
      expect(recordingIds.size).toBe(3);
      for (const id of recordingIds) {
        expect(uuid.test(id)).toBe(true);
        expect(
          (
            await read<Schema["Source"][]>(
              page,
              `/api/v1/recordings/${id}/sources`,
            )
          ).length,
        ).toBe(0);
      }
      expect(fixture.counts()).toEqual({
        manifest: 2,
        reference: 2,
        canonical: 4,
        items: 3,
        audio: 0,
        sources: 0,
        other: 0,
        invalid: 0,
        credentials: 0,
      });
      expect([offers, imports, follows, refreshes, privateEdits]).toEqual([
        1, 1, 1, 1, 2,
      ]);
      expect([pageErrors, externalRequests, audioRequests]).toEqual([0, 0, 0]);
      console.log(
        JSON.stringify({
          phase,
          copied_entries: copyBefore.entries.length,
          followed_entries_before: before.entries.length,
          followed_entries_after: after.entries.length,
          stable_witnessed_entries: 3,
          private_edits: privateEdits,
          refreshes,
          ...fixture.counts(),
        }),
      );
    } catch {
      failed = true;
    } finally {
      try {
        await fixture.close();
      } catch {
        failed = true;
        phase = 7;
      }
    }
    if (failed)
      throw new Error(`The isolated Follow workflow failed at phase ${phase}`);
  });
});

async function openPlaylists(page: Page) {
  await page
    .getByRole("navigation", { name: "Main navigation" })
    .getByRole("link", { name: "Playlists", exact: true })
    .click();
}

function postResponse(page: Page, path: string) {
  return page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname === path &&
      response.request().method() === "POST",
  );
}

async function read<T>(page: Page, path: string): Promise<T> {
  const response = await page.request.get(path);
  if (response.status() !== 200)
    throw new Error(`Owned Follow read returned HTTP ${response.status()}`);
  return response.json();
}

async function retainedTitles(table: Locator, titles: string[]) {
  const actual = await table
    .locator("tbody .track-title, tbody .retained-title")
    .allTextContents();
  samePrivateState(actual, titles);
}

function samePrivateState(actual: unknown, expected: unknown) {
  expect(JSON.stringify(actual) === JSON.stringify(expected)).toBe(true);
}
