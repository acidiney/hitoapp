import type { Schema } from "../../../shared/schema";
import { request, requestJson } from "../../../shared/http";

export const audiobookApi = {
  async read(
    account: string,
    playlist: string,
    signal: AbortSignal,
  ): Promise<Schema<"AudiobookDetail">> {
    signal.throwIfAborted();
    const response = await request(
      `/audiobooks/${encodeURIComponent(playlist)}`,
      {
        action: "audiobooks.read",
        account,
        signal,
      },
    );
    const detail: Schema<"AudiobookDetail"> = await response.json();
    signal.throwIfAborted();
    if (
      response.status !== 200 ||
      detail?.playlist?.id !== playlist ||
      detail.playlist.kind !== "local" ||
      typeof detail.playlist.name !== "string" ||
      !Array.isArray(detail.chapters) ||
      !Array.isArray(detail.bookmarks)
    )
      throw new Error("Hito could not verify this audiobook.");
    return detail;
  },
  playback(
    account: string,
    book: string,
    signal: AbortSignal,
  ): Promise<Schema<"AudiobookPlayback">> {
    return requestJson<Schema<"AudiobookPlayback">>(
      `/audiobooks/${encodeURIComponent(book)}/playback`,
      { action: "audiobooks.playback_read", account, signal },
    );
  },
};
