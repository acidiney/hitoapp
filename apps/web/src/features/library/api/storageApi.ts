import { request, requestJson } from "../../../shared/http";
import type {
  DirectoryPage,
  DirectoryRef,
  Job,
  MusicFolder,
  SharedDirectory,
} from "../../../shared/types";

const part = encodeURIComponent;
export const storageApi = {
  async directories(
    account: string,
    signal: AbortSignal,
  ): Promise<SharedDirectory[]> {
    return requestJson<SharedDirectory[]>("/storage/directories", {
      action: "folders.directories",
      account,
      signal,
    });
  },
  async browse(
    account: string,
    directory: DirectoryRef,
    offset: number | null,
    signal: AbortSignal,
  ): Promise<DirectoryPage> {
    return requestJson<DirectoryPage>("/storage/browse", {
      action: "folders.browse",
      method: "POST",
      account,
      body: { directory, offset },
      signal,
    });
  },
  async folders(
    account: string,
    library: string,
    signal: AbortSignal,
  ): Promise<MusicFolder[]> {
    return requestJson<MusicFolder[]>(`/libraries/${part(library)}/folders`, {
      action: "folders.list",
      account,
      signal,
    });
  },
  async connect(
    account: string,
    library: string,
    directory: DirectoryRef,
    signal: AbortSignal,
  ): Promise<MusicFolder> {
    return requestJson<MusicFolder>(`/libraries/${part(library)}/folders`, {
      action: "folders.connect",
      method: "POST",
      account,
      body: { directory },
      signal,
    });
  },
  async remove(
    account: string,
    folder: MusicFolder,
    signal: AbortSignal,
  ): Promise<void> {
    await request(`/folders/${part(folder.id)}`, {
      action: "folders.remove",
      method: "DELETE",
      account,
      body: { revision: folder.revision },
      signal,
    });
  },
  async scan(
    account: string,
    folder: string,
    signal: AbortSignal,
  ): Promise<Job> {
    return requestJson<Job>(`/folders/${part(folder)}/scan`, {
      action: "folders.scan",
      method: "POST",
      account,
      signal,
    });
  },
};
