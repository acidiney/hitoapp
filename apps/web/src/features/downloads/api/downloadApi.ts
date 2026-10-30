import type { Schema } from "../../../shared/schema";
import { HttpError, request, requestJson } from "../../../shared/http";

export const downloadApi = {
  async create(
    account: string,
    recording: string,
    body: Readonly<Schema<"DownloadRequest">>,
    signal: AbortSignal,
  ): Promise<Schema<"DownloadReceipt">> {
    return requestJson<Schema<"DownloadReceipt">>(
      `/recordings/${encodeURIComponent(recording)}/download`,
      {
        action: "downloads.create",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
  async status(
    account: string,
    recording: string,
    signal: AbortSignal,
  ): Promise<Schema<"DownloadStatus"> | null> {
    try {
      return await requestJson<Schema<"DownloadStatus"> | null>(
        `/recordings/${encodeURIComponent(recording)}/download`,
        {
          action: "downloads.status",
          account,
          signal,
        },
      );
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
