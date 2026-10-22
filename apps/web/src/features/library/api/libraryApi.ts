import type { Job, RefreshSources, Source } from "../../../shared/types";
import { requestJson } from "../../../shared/http";

const part = encodeURIComponent;

export const libraryApi = {
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
  async scan(
    account: string,
    library: string,
    signal?: AbortSignal,
  ): Promise<Job[]> {
    return requestJson<Job[]>(`/libraries/${part(library)}/scan`, {
      action: "scan.enqueue",
      method: "POST",
      account,
      signal,
    });
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
  async job(account: string, job: string, signal?: AbortSignal): Promise<Job> {
    return requestJson<Job>(`/jobs/${part(job)}`, {
      action: "job.status",
      account,
      signal,
    });
  },
};
