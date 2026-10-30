import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";

const part = encodeURIComponent;

export const sourcesApi = {
  async sources(
    account: string,
    recording: string,
    signal?: AbortSignal,
  ): Promise<Schema<"Source">[]> {
    return requestJson<Schema<"Source">[]>(
      `/recordings/${part(recording)}/sources`,
      {
        action: "sources.list",
        account,
        signal,
      },
    );
  },
  async playbackSources(
    account: string,
    recording: string,
    signal?: AbortSignal,
  ): Promise<Schema<"Source">[]> {
    return requestJson<Schema<"Source">[]>(
      `/recordings/${part(recording)}/playback-sources`,
      {
        action: "sources.playback",
        account,
        signal,
      },
    );
  },
  async refreshSources(
    account: string,
    recording: string,
    signal?: AbortSignal,
  ): Promise<Schema<"Source">[]> {
    return requestJson<Schema<"Source">[]>(
      `/recordings/${part(recording)}/sources/refresh`,
      {
        action: "sources.refresh",
        method: "POST",
        account,
        signal,
      },
    );
  },
  async recoverSources(
    account: string,
    recording: string,
    signal?: AbortSignal,
  ): Promise<Schema<"Source">[]> {
    return requestJson<Schema<"Source">[]>(
      `/recordings/${part(recording)}/sources/refresh`,
      {
        action: "sources.recover",
        method: "POST",
        body: { purpose: "playback" } satisfies Schema<"RefreshSources">,
        account,
        signal,
      },
    );
  },
};
