import { artistEditionApi } from "../api/artistEditionApi";
import type { CatalogEditionSummary } from "../../../shared/types";
import type { ArtistEditionContext } from "./useArtistEditions";
import { useEditionTrackRead } from "./useEditionTrackRead";

export function useArtistEditionTrackList(
  context: ArtistEditionContext,
  chosen: CatalogEditionSummary | null,
) {
  const { account, recording, association, group } = context;
  const { result, ...state } = useEditionTrackRead(
    context,
    chosen,
    null,
    (signal) =>
      artistEditionApi.tracks(
        account,
        recording,
        association.id,
        { release_group_id: group.id, edition_id: chosen!.id },
        signal,
      ),
  );
  return { ...state, edition: result?.edition ?? null };
}
