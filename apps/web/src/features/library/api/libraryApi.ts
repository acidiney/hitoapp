import type { Job } from "../../../shared/types";
import { requestJson } from "../../../shared/http";

const part = encodeURIComponent;

export const libraryApi = {
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
  async job(account: string, job: string, signal?: AbortSignal): Promise<Job> {
    return requestJson<Job>(`/jobs/${part(job)}`, {
      action: "job.status",
      account,
      signal,
    });
  },
};
