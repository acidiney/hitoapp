import { useEffect, useMemo, useRef } from "react";
import {
  useArtistEditions,
  type ArtistEditionContext,
} from "./useArtistEditions";
import { useArtistEditionTrackList } from "./useArtistEditionTrackList";
import { albumRows, creditText } from "../model/catalogAlbumRows";

export function useCatalogAlbum(context: ArtistEditionContext) {
  const editions = useArtistEditions(context, true);
  const { chosen, pending, choose } = editions;
  const first = editions.editions[0];
  const chooser = useRef(choose);
  chooser.current = choose;
  useEffect(() => {
    if (!chosen && first && !pending) chooser.current(first.id);
  }, [chosen, first, pending]);
  const tracks = useArtistEditionTrackList(context, chosen);
  const credit = creditText(context.group.artist_credit);
  const rows = useMemo(
    () => (tracks.edition ? albumRows(tracks.edition, credit) : []),
    [tracks.edition, credit],
  );
  return { editions, tracks, rows, credit };
}
