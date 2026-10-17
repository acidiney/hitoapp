import { HttpError, request } from "./http";
import type { DownloadReceipt, DownloadRequest, DownloadStatus } from "./types";

export const downloadApi = {
  async create(
    account: string,
    recording: string,
    body: Readonly<DownloadRequest>,
    signal: AbortSignal,
  ): Promise<DownloadReceipt> {
    return (
      await request(`/recordings/${encodeURIComponent(recording)}/download`, {
        action: "downloads.create",
        method: "POST",
        account,
        body,
        signal,
      })
    ).json();
  },
  async status(
    account: string,
    recording: string,
    signal: AbortSignal,
  ): Promise<DownloadStatus | null> {
    try {
      return await (
        await request(`/recordings/${encodeURIComponent(recording)}/download`, {
          action: "downloads.status",
          account,
          signal,
        })
      ).json();
    } catch (error) {
      if (error instanceof HttpError && error.status === 404) return null;
      throw error;
    }
  },
  async remove(account: string, intent: string, signal: AbortSignal) {
    const response = await request(`/downloads/${encodeURIComponent(intent)}`, {
      action: "downloads.remove",
      method: "DELETE",
      account,
      signal,
    });
    if (response.status !== 204)
      throw new Error("Hito did not confirm download removal");
  },
};
