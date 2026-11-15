import {
  trackListing,
  type ListingPage,
  type TrackListing,
} from "../../playback/api/trackListing";
import { request, requestJson } from "../../../shared/http";
import { chosenAvailability } from "../../../shared/hooks/useTrackAvailability";
import { TRACK_SORTS, chosenSort } from "../../../shared/hooks/useCatalogSort";
import type { Schema } from "../../../shared/schema";

export const UNCONFIRMED_FILTER =
  "This server didn't apply the filter. Update the server to use it.";

export async function confirmedRecordings(
  response: Response,
  availability: string | null,
): Promise<Schema<"RecordingPage">> {
  if (
    availability &&
    response.headers.get("Hito-Recordings-Availability") !== availability
  )
    throw new Error(UNCONFIRMED_FILTER);
  return response.json() as Promise<Schema<"RecordingPage">>;
}

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
  else filters.set("sort", chosenSort(TRACK_SORTS));
  for (const key of ["library", "artist"]) {
    const value = location.get(key);
    if (value) filters.set(key, value);
  }
  const availability = liked ? null : chosenAvailability();
  if (availability) filters.set("availability", availability);
  return trackListing(page, async (offset, limit, signal) => {
    const params = new URLSearchParams(filters);
    params.set("offset", String(offset));
    params.set("limit", String(limit));
    const result = await confirmedRecordings(
      await request(`/recordings?${params}`, {
        action: "catalog.tracks_page",
        account,
        signal,
      }),
      availability,
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
