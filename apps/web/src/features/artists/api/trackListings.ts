import type { TrackListing } from "../../playback/api/trackListing";
import { requestJson } from "../../../shared/http";
import type { components } from "../../../generated";

type Schema = components["schemas"];
type Page = { total: number; offset: number };

export function artistTracksListing(
  account: string,
  artist: string,
  page: Page,
): TrackListing {
  return {
    total: page.total,
    offset: page.offset,
    async fetch(offset, limit, signal) {
      const params = new URLSearchParams({
        recording_offset: String(offset),
        limit: String(limit),
      });
      const result = await requestJson<Schema["ArtistDetail"]>(
        `/artists/${encodeURIComponent(artist)}?${params}`,
        { action: "artist.tracks_page", account, signal },
      );
      return { total: result.recordings.total, items: result.recordings.items };
    },
  };
}

export function artistGroupTracksListing(
  account: string,
  group: string,
  page: Page,
): TrackListing {
  const library = new URLSearchParams(window.location.search).get("library");
  return {
    total: page.total,
    offset: page.offset,
    async fetch(offset, limit, signal) {
      const params = new URLSearchParams();
      if (library) params.set("library", library);
      params.set("recording_offset", String(offset));
      params.set("limit", String(limit));
      const result = await requestJson<Schema["ArtistIdentityGroupDetail"]>(
        `/artist-identity-groups/${encodeURIComponent(group)}?${params}`,
        { action: "artist.group_tracks_page", account, signal },
      );
      return {
        total: result.recordings.total,
        items: result.recordings.items.map(({ recording }) => recording),
      };
    },
  };
}
