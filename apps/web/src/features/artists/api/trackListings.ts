import {
  trackListing,
  type ListingPage,
  type TrackListing,
} from "../../playback/api/trackListing";
import { requestJson } from "../../../shared/http";
import type { Schema } from "../../../shared/schema";

export function artistTracksListing(
  account: string,
  artist: string,
  page: ListingPage,
): TrackListing {
  return trackListing(page, async (offset, limit, signal) => {
    const params = new URLSearchParams({
      recording_offset: String(offset),
      limit: String(limit),
    });
    const result = await requestJson<Schema<"ArtistDetail">>(
      `/artists/${encodeURIComponent(artist)}?${params}`,
      { action: "artist.tracks_page", account, signal },
    );
    return { total: result.recordings.total, items: result.recordings.items };
  });
}

export async function artistAppearancesPage(
  account: string,
  artist: string,
  offset: number,
  limit: number,
  signal: AbortSignal,
): Promise<Schema<"Page_Recording">> {
  const params = new URLSearchParams({
    featured_offset: String(offset),
    limit: String(limit),
  });
  const result = await requestJson<Schema<"ArtistDetail">>(
    `/artists/${encodeURIComponent(artist)}?${params}`,
    { action: "artist.appearances_page", account, signal },
  );
  return result.featured_recordings;
}

export function artistAppearancesListing(
  account: string,
  artist: string,
  page: ListingPage,
): TrackListing {
  return trackListing(page, async (offset, limit, signal) => {
    const result = await artistAppearancesPage(
      account,
      artist,
      offset,
      limit,
      signal,
    );
    return { total: result.total, items: result.items };
  });
}

export function artistGroupTracksListing(
  account: string,
  group: string,
  page: ListingPage,
): TrackListing {
  const library = new URLSearchParams(window.location.search).get("library");
  return trackListing(page, async (offset, limit, signal) => {
    const params = new URLSearchParams();
    if (library) params.set("library", library);
    params.set("recording_offset", String(offset));
    params.set("limit", String(limit));
    const result = await requestJson<Schema<"ArtistIdentityGroupDetail">>(
      `/artist-identity-groups/${encodeURIComponent(group)}?${params}`,
      { action: "artist.group_tracks_page", account, signal },
    );
    return {
      total: result.recordings.total,
      items: result.recordings.items.map(({ recording }) => recording),
    };
  });
}
