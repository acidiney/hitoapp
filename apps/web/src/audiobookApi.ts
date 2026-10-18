import { request } from "./http";
import type { AudiobookDetail } from "./types";

export const audiobookApi = {
  async read(
    account: string,
    playlist: string,
    signal: AbortSignal,
  ): Promise<AudiobookDetail> {
    signal.throwIfAborted();
    const response = await request(
      `/audiobooks/${encodeURIComponent(playlist)}`,
      {
        action: "audiobooks.read",
        account,
        signal,
      },
    );
    const detail: AudiobookDetail = await response.json();
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
};
