import type { Schema } from "../../../shared/schema";
import { useEffect, useMemo, useRef } from "react";
import { useArtistDiscography } from "../../artists/hooks/useArtistDiscography";
import { matchReleases } from "../model/releaseMatch";

const MAX_SEARCH_PAGES = 10;

export function useReleaseSuggestion(
  account: string,
  recording: string,
  association: Schema<"ArtistAssociation">,
  profile: Schema<"CatalogArtistProfile">,
  title: string,
  year: number | null,
) {
  const discography = useArtistDiscography(
    account,
    recording,
    association,
    profile,
    true,
  );
  const moreLatest = useRef(discography.more);
  moreLatest.current = discography.more;
  const match = useMemo(
    () => matchReleases(discography.groups, title, year),
    [discography.groups, title, year],
  );
  const failed =
    discography.error !== "" && !discography.retrying && !discography.pending;
  const searching =
    match.suggestion === null &&
    discography.continuation !== null &&
    discography.pages < MAX_SEARCH_PAGES &&
    !discography.error;
  useEffect(() => {
    if (searching && discography.pages > 0 && !discography.pending)
      void moreLatest.current();
  }, [searching, discography.pages, discography.pending]);
  return {
    groups: discography.groups,
    suggestion: match.suggestion,
    unique: match.unique,
    settled: discography.pages > 0 && !discography.pending && !searching,
    failed,
    error: discography.error,
    canLoadMore: discography.continuation !== null,
    pending: discography.pending,
    more: discography.more,
    retry: () =>
      discography.restartRequired ? discography.restart() : discography.retry(),
  };
}
