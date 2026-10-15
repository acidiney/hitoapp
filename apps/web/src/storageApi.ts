import { request } from "./http";
import type {
  DirectoryPage,
  DirectoryRef,
  Job,
  MusicFolder,
  SharedDirectory,
} from "./types";

const part = encodeURIComponent;
export const storageApi = {
  async directories(
    account: string,
    signal: AbortSignal,
  ): Promise<SharedDirectory[]> {
    return (
      await request("/storage/directories", {
        action: "folders.directories",
        account,
        signal,
      })
    ).json();
  },
  async browse(
    account: string,
    directory: DirectoryRef,
    offset: number | null,
    signal: AbortSignal,
  ): Promise<DirectoryPage> {
    return (
      await request("/storage/browse", {
        action: "folders.browse",
        method: "POST",
        account,
        body: { directory, offset },
        signal,
      })
    ).json();
  },
  async folders(
    account: string,
    library: string,
    signal: AbortSignal,
  ): Promise<MusicFolder[]> {
    return (
      await request(`/libraries/${part(library)}/folders`, {
        action: "folders.list",
        account,
        signal,
      })
    ).json();
  },
  async connect(
    account: string,
    library: string,
    directory: DirectoryRef,
    signal: AbortSignal,
  ): Promise<MusicFolder> {
    return (
      await request(`/libraries/${part(library)}/folders`, {
        action: "folders.connect",
        method: "POST",
        account,
        body: { directory },
        signal,
      })
    ).json();
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
    return (
      await request(`/folders/${part(folder)}/scan`, {
        action: "folders.scan",
        method: "POST",
        account,
        signal,
      })
    ).json();
  },
};
