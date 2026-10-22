import { request } from "../../../shared/http";
import type {
  EditionTrackListAvailability,
  ReadEditionTrackList,
  ReadReleaseGroupEditions,
  ReleaseGroupEditionsAvailability,
} from "../../../shared/types";

const part = encodeURIComponent;
const INVALID_RESPONSE =
  "Hito returned an invalid edition response. Try again.";
const availabilityStatuses: Record<
  | ReleaseGroupEditionsAvailability["status"]
  | EditionTrackListAvailability["status"],
  true
> = {
  available: true,
  disabled: true,
  unsupported: true,
  unavailable: true,
  cancelled: true,
};

async function json<
  T extends ReleaseGroupEditionsAvailability | EditionTrackListAvailability,
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
    body: ReadReleaseGroupEditions,
    signal: AbortSignal,
  ): Promise<ReleaseGroupEditionsAvailability> {
    return json<ReleaseGroupEditionsAvailability>(
      await request(
        `/recordings/${part(recording)}/artist-associations/${part(association)}/release-group-editions`,
        {
          action: "artist_editions.read",
          method: "POST",
          account,
          body,
          signal,
        },
      ),
    );
  },
  async tracks(
    account: string,
    recording: string,
    association: string,
    body: ReadEditionTrackList,
    signal: AbortSignal,
  ): Promise<EditionTrackListAvailability> {
    return json<EditionTrackListAvailability>(
      await request(
        `/recordings/${part(recording)}/artist-associations/${part(association)}/edition-track-list`,
        {
          action: "artist_edition_track_list.read",
          method: "POST",
          account,
          body,
          signal,
        },
      ),
    );
  },
};
