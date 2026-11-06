import type { Schema } from "../../../shared/schema";
import { request } from "../../../shared/http";
import { audioOutcome } from "./albumEditionApi";
import { cachedRead } from "./catalogCache";

const part = encodeURIComponent;
const INVALID_RESPONSE =
  "Hito returned an invalid edition response. Try again.";
const availabilityStatuses: Record<
  | Schema<"ReleaseGroupEditionsAvailability">["status"]
  | Schema<"EditionTrackListAvailability">["status"],
  true
> = {
  available: true,
  disabled: true,
  unsupported: true,
  unavailable: true,
  cancelled: true,
};

async function json<
  T extends
    | Schema<"ReleaseGroupEditionsAvailability">
    | Schema<"EditionTrackListAvailability">,
>(response: Response): Promise<T> {
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
    !Object.hasOwn(availabilityStatuses, value.status)
  )
    throw new Error(INVALID_RESPONSE);
  return value as T;
}

export const artistEditionApi = {
  async editions(
    account: string,
    recording: string,
    association: string,
    body: Schema<"ReadReleaseGroupEditions">,
    signal: AbortSignal,
    fresh = false,
  ): Promise<Schema<"ReleaseGroupEditionsAvailability">> {
    return cachedRead(
      account,
      ["editions", recording, association, body],
      async (loadSignal) =>
        json<Schema<"ReleaseGroupEditionsAvailability">>(
          await request(
            `/recordings/${part(recording)}/artist-associations/${part(association)}/release-group-editions`,
            {
              action: "artist_editions.read",
              method: "POST",
              account,
              body,
              signal: loadSignal,
            },
          ),
        ),
      signal,
      fresh,
    );
  },
  async tracks(
    account: string,
    recording: string,
    association: string,
    body: Schema<"ReadEditionTrackList">,
    signal: AbortSignal,
  ): Promise<Schema<"EditionTrackListAvailability">> {
    return cachedRead(
      account,
      ["tracks", recording, association, body],
      async (loadSignal) =>
        json<Schema<"EditionTrackListAvailability">>(
          await request(
            `/recordings/${part(recording)}/artist-associations/${part(association)}/edition-track-list`,
            {
              action: "artist_edition_track_list.read",
              method: "POST",
              account,
              body,
              signal: loadSignal,
            },
          ),
        ),
      signal,
    );
  },
  async audio(
    account: string,
    recording: string,
    association: string,
    body: Schema<"FindArtistTrackAudio">,
    signal: AbortSignal,
  ): Promise<Schema<"AlbumTrackAudio">> {
    return audioOutcome(
      await request(
        `/recordings/${part(recording)}/artist-associations/${part(association)}/edition-track-audio`,
        {
          action: "artist_edition_track_audio.find",
          method: "POST",
          account,
          body,
          signal,
        },
      ),
    );
  },
};
