import {
  trackListing,
  type ListingPage,
  type TrackListing,
} from "../../playback/api/trackListing";
import { requestJson } from "../../../shared/http";
import type { Schema } from "../../../shared/schema";

export function recordingsListing(
  account: string,
  page: ListingPage,
  query: string,
  liked: boolean,
): TrackListing {
  const location = new URLSearchParams(window.location.search);
  const filters = new URLSearchParams();
  if (query) filters.set("q", query);
  if (liked) filters.set("liked", "true");
  for (const key of ["library", "artist"]) {
    const value = location.get(key);
    if (value) filters.set(key, value);
  }
  return trackListing(page, async (offset, limit, signal) => {
    const params = new URLSearchParams(filters);
    params.set("offset", String(offset));
    params.set("limit", String(limit));
    const result = await requestJson<Schema<"RecordingPage">>(
      `/recordings?${params}`,
      { action: "catalog.tracks_page", account, signal },
    );
    return { total: result.total, items: result.items };
  });
}

export function releaseTracksListing(
  account: string,
  release: string,
  page: ListingPage,
): TrackListing {
  return trackListing(page, async (offset, limit, signal) => {
    const params = new URLSearchParams({
      offset: String(offset),
      limit: String(limit),
    });
    const result = await requestJson<Schema<"ReleaseDetail">>(
      `/releases/${encodeURIComponent(release)}?${params}`,
      { action: "catalog.album_tracks_page", account, signal },
    );
    return { total: result.tracks.total, items: result.tracks.items };
  });
}
