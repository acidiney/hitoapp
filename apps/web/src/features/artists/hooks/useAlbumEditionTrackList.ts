import { albumEditionApi } from "../api/albumEditionApi";
import type {
  AlbumEditionTrackListAvailability,
  CatalogEditionSummary,
} from "../../../shared/types";
import type { ArtistEditionContext } from "./useArtistEditions";
import { useEditionTrackRead } from "./useEditionTrackRead";

type Available = Extract<
  AlbumEditionTrackListAvailability,
  { status: "available" }
>;

export function useAlbumEditionTrackList(
  context: ArtistEditionContext,
  release: string,
  chosen: CatalogEditionSummary | null,
) {
  const { account, recording, association, group } = context;
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
          release_group_id: group.id,
          edition_id: chosen!.id,
        },
        signal,
      ),
  );
  return { ...state, comparison: result };
}
