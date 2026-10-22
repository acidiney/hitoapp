import { requestJson } from "../../../shared/http";
import type { RefreshSources, Source } from "../../../shared/types";

const part = encodeURIComponent;

export const sourcesApi = {
  async sources(
    account: string,
    recording: string,
    signal?: AbortSignal,
  ): Promise<Source[]> {
    return requestJson<Source[]>(`/recordings/${part(recording)}/sources`, {
      action: "sources.list",
      account,
      signal,
    });
  },
  async playbackSources(
    account: string,
    recording: string,
    signal?: AbortSignal,
  ): Promise<Source[]> {
    return requestJson<Source[]>(
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
  ): Promise<Source[]> {
    return requestJson<Source[]>(
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
  ): Promise<Source[]> {
    return requestJson<Source[]>(
      `/recordings/${part(recording)}/sources/refresh`,
      {
        action: "sources.recover",
        method: "POST",
        body: { purpose: "playback" } satisfies RefreshSources,
        account,
        signal,
      },
    );
  },
};
