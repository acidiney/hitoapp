import { randomUUID } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { expect, type Page } from "@playwright/test";
import { liveCheck } from "./spotify-albums-live";
import {
  upgradePost,
  upgradeReplay,
  type UpgradeSchema,
} from "./spotify-metadata-upgrade-state";
import type { PreparedSpotifyUpgrade } from "./spotify-metadata-upgrade-setup";

export async function editSpotifyUpgradeCopy(
  page: Page,
  account: string,
  playlist: string,
  selected: string,
  entryId: string,
  entryCount: number,
) {
  const personal = {
    title: `Owned private title ${randomUUID()}`,
    artist: `Owned private artist ${randomUUID()}`,
    playlist: `Owned private playlist ${randomUUID()}`,
  };
  const actions: UpgradeSchema["Action"][] = [
    {
      kind: "recording_title_set",
      recording_id: selected,
      title: personal.title,
    },
    {
      kind: "recording_artist_set",
      recording_id: selected,
      artist: personal.artist,
    },
    { kind: "like_set", recording_id: selected, liked: true },
    {
      kind: "progress_set",
      recording_id: selected,
      position_ms: 4200,
      speed: 1.25,
    },
    { kind: "playlist_rename", playlist_id: playlist, name: personal.playlist },
    {
      kind: "entry_move",
      playlist_id: playlist,
      entry_id: entryId,
      position: entryCount + 10,
    },
  ];
  const edits: UpgradeSchema["SyncBatch"] = {
    device_id: "spotify-metadata-upgrade-live",
    operations: actions.map((action) => ({ id: randomUUID(), action })),
  };
  const response = await page.request.post("/api/v1/sync", {
    headers: { "x-hito-request": "1", "x-hito-account": account },
    data: edits,
    timeout: 10_000,
    maxRedirects: 0,
    maxRetries: 0,
  });
  let editReceipt: Buffer;
  try {
    liveCheck(response.status() === 200);
    editReceipt = await response.body();
    liveCheck(
      editReceipt.length <= 32_768 &&
        isDeepStrictEqual(
          (JSON.parse(editReceipt.toString()) as UpgradeSchema["SyncReceipt"])
            .acknowledged,
          edits.operations.map((operation) => operation.id),
        ),
    );
  } finally {
    await response.dispose();
  }
  return { personal, edits, editReceipt };
}

export async function findMissingSpotifyAlbums(
  page: Page,
  prepared: PreparedSpotifyUpgrade,
) {
  const panel = page.getByRole("region", {
    name: "Playlist metadata",
    exact: true,
  });
  const repaired = await upgradePost<
    UpgradeSchema["ImportedPlaylistMetadataRefresh"]
  >(page, `/api/v1/playlists/${prepared.playlist}/metadata/refresh`, () =>
    panel
      .getByRole("button", { name: "Find missing albums", exact: true })
      .click(),
  );
  const sent =
    repaired.sent as UpgradeSchema["RefreshImportedPlaylistMetadata"];
  liveCheck(
    Object.keys(sent).sort().join(",") === "installation_revision,request_id" &&
      /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(
        sent.request_id,
      ) &&
      sent.request_id !== "00000000-0000-0000-0000-000000000000" &&
      sent.installation_revision === prepared.connection.revision &&
      repaired.body.request_id === sent.request_id,
  );
  const receipt = repaired.body;
  const status = `${receipt.updated_recordings} ${receipt.updated_recordings === 1 ? "recording" : "recordings"} updated. ${receipt.unchanged_recordings} unchanged. ${receipt.unavailable_recordings} unavailable.`;
  await expect
    .poll(
      async () =>
        (await panel.getByRole("status").textContent())
          ?.replace(/\s+/g, " ")
          .trim() === status,
      { timeout: 8000 },
    )
    .toBe(true);
  return { ...repaired, sent };
}

export async function replaySpotifyUpgradeReceipts(
  page: Page,
  prepared: PreparedSpotifyUpgrade,
  repaired: Awaited<ReturnType<typeof findMissingSpotifyAlbums>>,
) {
  let completed = 0;
  for (const replay of [
    {
      path: `/api/v1/plugins/${prepared.connection.id}/playlists/import`,
      data: prepared.importRequest,
      bytes: prepared.imported.bytes,
    },
    { path: "/api/v1/sync", data: prepared.edits, bytes: prepared.editReceipt },
    {
      path: `/api/v1/playlists/${prepared.playlist}/metadata/refresh`,
      data: repaired.sent,
      bytes: repaired.bytes,
    },
  ]) {
    await upgradeReplay(
      page,
      prepared.user.id,
      replay.path,
      replay.data,
      replay.bytes,
    );
    completed++;
  }
  return completed;
}
