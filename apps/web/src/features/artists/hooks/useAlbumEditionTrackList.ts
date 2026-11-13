import type { Schema } from "../../../shared/schema";
import { albumEditionApi } from "../api/albumEditionApi";

import type { ArtistEditionContext } from "./useArtistEditions";
import { useEditionTrackRead } from "./useEditionTrackRead";

type Available = Extract<
  Schema<"AlbumEditionTrackListAvailability">,
  { status: "available" }
>;

export function useAlbumEditionTrackList(
  context: ArtistEditionContext,
  release: string,
  chosen: Schema<"CatalogEditionSummary"> | null,
) {
  const { account, recording, association, releaseGroupId } = context;
  const { result, ...state } = useEditionTrackRead<Available>(
    context,
    chosen,
    release,
    (signal) =>
      albumEditionApi.tracks(
        account,
        release,
        {
          recording_id: recording,
          association_id: association.id,
          release_group_id: releaseGroupId,
          edition_id: chosen!.id,
        },
        signal,
      ),
  );
  return { ...state, comparison: result };
}
