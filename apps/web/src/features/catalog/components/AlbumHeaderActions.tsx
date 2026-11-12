import { useContext } from "react";
import { Link, router } from "@inertiajs/react";
import type { Track } from "../../playback/model";
import type { TrackListing } from "../../playback/api/trackListing";
import ActionMenu, { type ActionMenuItem } from "../../../shared/ui/ActionMenu";
import {
  PHONE_QUERY,
  useMediaQuery,
} from "../../../shared/hooks/useMediaQuery";
import ArtworkRetryMenu, {
  useArtworkRetryItem,
} from "../../artwork/components/ArtworkRetryMenu";
import { usePlayTracks } from "../../playback/hooks/usePlayTracks";
import AlbumPlayActions from "./AlbumPlayActions";
import { AlbumMergeContext } from "./AlbumMergeContext";

export default function AlbumHeaderActions({
  release,
  tracks,
  listing,
}: {
  release: string;
  tracks: Track[];
  listing: TrackListing;
}) {
  const phone = useMediaQuery(PHONE_QUERY);
  const detailsHref = `/albums/${encodeURIComponent(release)}/details`;
  const retry = useArtworkRetryItem({ release });
  const play = usePlayTracks(tracks, listing);
  const { editionPlay: edition } = useContext(AlbumMergeContext);
  const items: ActionMenuItem[] = [
    {
      label: "Shuffle album",
      disabled: edition ? edition.tracks === 0 : tracks.length === 0,
      onSelect: () => void (edition ? edition.shuffle() : play.shuffleAll()),
    },
    { label: "Edit album details", onSelect: () => router.visit(detailsHref) },
    ...(retry ? [retry] : []),
  ];
  return (
    <>
      <AlbumPlayActions
        tracks={tracks}
        listing={listing}
        shuffle={!phone}
        edition={edition}
      />
      {phone ? (
        <ActionMenu
          label="More actions"
          triggerClassName="pill-icon"
          align="end"
          items={items}
        />
      ) : (
        <>
          <Link className="pill-action" href={detailsHref}>
            Edit album details
          </Link>
          <ArtworkRetryMenu release={release} />
        </>
      )}
    </>
  );
}
