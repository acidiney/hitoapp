import { request } from "../../../shared/http";
import type {
  AlbumEditionTrackListAvailability,
  ReadAlbumEditionTrackList,
} from "../../../shared/types";

const INVALID_RESPONSE =
  "Hito returned an invalid edition response. Try again.";
const statuses: Record<AlbumEditionTrackListAvailability["status"], true> = {
  available: true,
  disabled: true,
  unsupported: true,
  unavailable: true,
  cancelled: true,
  needs_choice: true,
};

export async function availability(
  response: Response,
): Promise<AlbumEditionTrackListAvailability> {
  let value: unknown;
  try {
    value = await response.json();
  } catch (error) {
    if (error instanceof SyntaxError) throw new Error(INVALID_RESPONSE);
    throw error;
  }
  if (
    value === null ||
    typeof value !== "object" ||
    !("status" in value) ||
    typeof value.status !== "string" ||
    !Object.hasOwn(statuses, value.status)
  )
    throw new Error(INVALID_RESPONSE);
  return value as AlbumEditionTrackListAvailability;
}

export const albumEditionApi = {
  async tracks(
    account: string,
    release: string,
    body: ReadAlbumEditionTrackList,
    signal: AbortSignal,
  ): Promise<AlbumEditionTrackListAvailability> {
    const response = await request(
      `/albums/${encodeURIComponent(release)}/edition-track-list`,
      {
        action: "album_edition_track_list.read",
        method: "POST",
        account,
        body,
        signal,
      },
    );
    return availability(response);
  },
};
