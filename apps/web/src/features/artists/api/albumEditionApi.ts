import { request } from "../../../shared/http";
import type {
  AlbumEditionTrackListAvailability,
  AlbumTrackAudio,
  FindAlbumTrackAudio,
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

const audioStatuses: Record<AlbumTrackAudio["status"], true> = {
  saved: true,
  no_match: true,
  insufficient_evidence: true,
  no_providers: true,
  busy: true,
  changed: true,
  disabled: true,
  unsupported: true,
  unavailable: true,
  cancelled: true,
  needs_choice: true,
};

async function statusBody(response: Response): Promise<{ status: string }> {
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
    typeof value.status !== "string"
  )
    throw new Error(INVALID_RESPONSE);
  return value as { status: string };
}

export async function availability(
  response: Response,
): Promise<AlbumEditionTrackListAvailability> {
  const value = await statusBody(response);
  if (!Object.hasOwn(statuses, value.status)) throw new Error(INVALID_RESPONSE);
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

  async audio(
    account: string,
    release: string,
    body: FindAlbumTrackAudio,
    signal: AbortSignal,
  ): Promise<AlbumTrackAudio> {
    const response = await request(
      `/albums/${encodeURIComponent(release)}/edition-track-audio`,
      {
        action: "album_edition_track_audio.find",
        method: "POST",
        account,
        body,
        signal,
      },
    );
    const value = await statusBody(response);
    if (!Object.hasOwn(audioStatuses, value.status))
      throw new Error(INVALID_RESPONSE);
    return value as AlbumTrackAudio;
  },
};
