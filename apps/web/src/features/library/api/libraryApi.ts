import type { Job, RefreshSources, Source } from "../../../shared/types";
import { request } from "../../../shared/http";

const part = encodeURIComponent;

export const libraryApi = {
  async sources(
    account: string,
    recording: string,
    signal?: AbortSignal,
  ): Promise<Source[]> {
    return (
      await request(`/recordings/${part(recording)}/sources`, {
        action: "sources.list",
        account,
        signal,
      })
    ).json();
  },
  async playbackSources(
    account: string,
    recording: string,
    signal?: AbortSignal,
  ): Promise<Source[]> {
    return (
      await request(`/recordings/${part(recording)}/playback-sources`, {
        action: "sources.playback",
        account,
        signal,
      })
    ).json();
  },
  async scan(
    account: string,
    library: string,
    signal?: AbortSignal,
  ): Promise<Job[]> {
    return (
      await request(`/libraries/${part(library)}/scan`, {
        action: "scan.enqueue",
        method: "POST",
        account,
        signal,
      })
    ).json();
  },
  async refreshSources(
    account: string,
    recording: string,
    signal?: AbortSignal,
  ): Promise<Source[]> {
    return (
      await request(`/recordings/${part(recording)}/sources/refresh`, {
        action: "sources.refresh",
        method: "POST",
        account,
        signal,
      })
    ).json();
  },
  async recoverSources(
    account: string,
    recording: string,
    signal?: AbortSignal,
  ): Promise<Source[]> {
    return (
      await request(`/recordings/${part(recording)}/sources/refresh`, {
        action: "sources.recover",
        method: "POST",
        body: { purpose: "playback" } satisfies RefreshSources,
        account,
        signal,
      })
    ).json();
  },
  async job(account: string, job: string, signal?: AbortSignal): Promise<Job> {
    return (
      await request(`/jobs/${part(job)}`, {
        action: "job.status",
        account,
        signal,
      })
    ).json();
  },
};
