import type { Schema } from "../../../shared/schema";
import { request, requestJson } from "../../../shared/http";

const part = encodeURIComponent;
export const storageApi = {
  async directories(
    account: string,
    signal: AbortSignal,
  ): Promise<Schema<"SharedDirectory">[]> {
    return requestJson<Schema<"SharedDirectory">[]>("/storage/directories", {
      action: "folders.directories",
      account,
      signal,
    });
  },
  async browse(
    account: string,
    directory: Schema<"DirectoryRef">,
    offset: number | null,
    signal: AbortSignal,
  ): Promise<Schema<"DirectoryPage">> {
    return requestJson<Schema<"DirectoryPage">>("/storage/browse", {
      action: "folders.browse",
      method: "POST",
      account,
      body: { directory, offset },
      signal,
    });
  },
  async hostOffer(
    account: string,
    signal: AbortSignal,
  ): Promise<Schema<"HostFolderOffer">> {
    return requestJson<Schema<"HostFolderOffer">>("/host/folders", {
      action: "folders.host_offer",
      account,
      signal,
    });
  },
  async chooseHostFolder(
    account: string,
    signal: AbortSignal,
  ): Promise<Schema<"HostFolderChoice">> {
    return requestJson<Schema<"HostFolderChoice">>("/host/folders/choose", {
      action: "folders.host_choose",
      method: "POST",
      account,
      signal,
    });
  },
  async folders(
    account: string,
    library: string,
    signal: AbortSignal,
  ): Promise<Schema<"MusicFolder">[]> {
    return requestJson<Schema<"MusicFolder">[]>(
      `/libraries/${part(library)}/folders`,
      {
        action: "folders.list",
        account,
        signal,
      },
    );
  },
  async connect(
    account: string,
    library: string,
    directory: Schema<"DirectoryRef">,
    signal: AbortSignal,
  ): Promise<Schema<"MusicFolder">> {
    return requestJson<Schema<"MusicFolder">>(
      `/libraries/${part(library)}/folders`,
      {
        action: "folders.connect",
        method: "POST",
        account,
        body: { directory },
        signal,
      },
    );
  },
  async remove(
    account: string,
    folder: Schema<"MusicFolder">,
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
  ): Promise<Schema<"Job">> {
    return requestJson<Schema<"Job">>(`/folders/${part(folder)}/scan`, {
      action: "folders.scan",
      method: "POST",
      account,
      signal,
    });
  },
  async removeMissingRecordings(
    account: string,
    library: string,
    signal: AbortSignal,
  ): Promise<Schema<"MissingRecordingsRemoval">> {
    return requestJson<Schema<"MissingRecordingsRemoval">>(
      `/libraries/${part(library)}/missing-recordings`,
      {
        action: "recordings.remove_missing",
        method: "DELETE",
        account,
        signal,
      },
    );
  },
};
