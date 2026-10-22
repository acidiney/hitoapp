import { HttpError, request } from "./shared/http";
import type {
  DownloadRequest,
  PlaylistDownloadReceipt,
  PlaylistDownloadStatus,
} from "./shared/types";

const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const states = new Set([
  "pending",
  "waiting_for_source",
  "queued",
  "running",
  "verifying",
  "ready",
  "failed",
  "unavailable",
]);

function validStatus(value: PlaylistDownloadStatus, playlist: string) {
  return (
    value?.playlist_id === playlist &&
    uuid.test(value.retention_id) &&
    typeof value.host_device_id === "string" &&
    value.host_device_id.length > 0 &&
    Number.isSafeInteger(value.desired_revision) &&
    value.desired_revision > 0 &&
    Number.isSafeInteger(value.reconciled_revision) &&
    value.reconciled_revision >= 0 &&
    value.reconciled_revision <= value.desired_revision &&
    Array.isArray(value.entries?.items) &&
    value.entries.items.length <= 32 &&
    (value.entries.next_cursor == null ||
      (value.entries.items.length > 0 &&
        typeof value.entries.next_cursor === "string" &&
        value.entries.next_cursor.length > 0 &&
        value.entries.next_cursor.length <= 128)) &&
    new Set(value.entries.items.map((entry) => entry?.entry_id)).size ===
      value.entries.items.length &&
    value.entries.items.every(
      (entry) =>
        uuid.test(entry?.entry_id) &&
        (entry.recording_id == null || uuid.test(entry.recording_id)) &&
        states.has(entry.status) &&
        (entry.source_id == null || uuid.test(entry.source_id)) &&
        (entry.status !== "ready" ||
          (entry.recording_id != null && entry.source_id != null)),
    )
  );
}

export const playlistDownloadApi = {
  async create(
    account: string,
    playlist: string,
    body: Readonly<DownloadRequest>,
    signal: AbortSignal,
  ): Promise<PlaylistDownloadReceipt> {
    const response = await request(
      `/playlists/${encodeURIComponent(playlist)}/download`,
      {
        action: "downloads.create",
        method: "POST",
        account,
        body,
        signal,
      },
    );
    if (response.status !== 202)
      throw new Error("Hito did not confirm playlist download creation");
    const receipt: PlaylistDownloadReceipt = await response.json();
    if (
      receipt?.request_id !== body.request_id ||
      receipt.playlist_id !== playlist ||
      !uuid.test(receipt.retention_id)
    )
      throw new Error("Hito returned a different playlist download receipt");
    return receipt;
  },
  async status(
    account: string,
    playlist: string,
    signal: AbortSignal,
  ): Promise<PlaylistDownloadStatus | null> {
    try {
      const status: PlaylistDownloadStatus = await (
        await request(`/playlists/${encodeURIComponent(playlist)}/download`, {
          action: "downloads.status",
          account,
          signal,
        })
      ).json();
      if (!validStatus(status, playlist))
        throw new Error("Hito returned an invalid playlist download status");
      return status;
    } catch (error) {
      if (error instanceof HttpError && error.status === 404) return null;
      throw error;
    }
  },
  async remove(account: string, retention: string, signal: AbortSignal) {
    const response = await request(
      `/downloads/${encodeURIComponent(retention)}`,
      {
        action: "downloads.remove",
        method: "DELETE",
        account,
        signal,
      },
    );
    if (response.status !== 204)
      throw new Error("Hito did not confirm playlist download removal");
  },
};
