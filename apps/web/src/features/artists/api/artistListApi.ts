import type { Schema } from "../../../shared/schema";
import { request } from "../../../shared/http";

export type ArtistListFilter = {
  query: string;
  library: string | null;
  limit: string | null;
};

function path(base: string, filter: ArtistListFilter, offset: number) {
  const parameters = new URLSearchParams();
  if (filter.query) parameters.set("q", filter.query);
  if (filter.library) parameters.set("library", filter.library);
  if (filter.limit) parameters.set("limit", filter.limit);
  parameters.set("offset", String(offset));
  return `${base}?${parameters}`;
}

async function read<T>(
  url: string,
  action: string,
  account: string,
  signal: AbortSignal,
): Promise<T> {
  const response = await request(url, { action, account, signal });
  if (signal.aborted) {
    await response.body?.cancel();
    signal.throwIfAborted();
  }
  return response.json();
}

export const artistListApi = {
  credits(
    account: string,
    filter: ArtistListFilter,
    offset: number,
    signal: AbortSignal,
  ) {
    return read<Schema<"ArtistPage">>(
      path("/artists", filter, offset),
      "artists.list",
      account,
      signal,
    );
  },
  groups(
    account: string,
    filter: ArtistListFilter,
    offset: number,
    signal: AbortSignal,
  ) {
    return read<Schema<"ArtistIdentityGroupPage">>(
      path("/artist-identity-groups", filter, offset),
      "artist_groups.list",
      account,
      signal,
    );
  },
};
