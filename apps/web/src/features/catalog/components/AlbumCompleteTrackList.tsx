import { useState } from "react";
import ArtistDiscography from "../../artists/components/ArtistDiscography";
import { useReviewedArtistProfile } from "../../artists/hooks/useReviewedArtistProfile";
import type { AlbumCatalogAnchor } from "../../../shared/types";

type Props = {
  account: string;
  release: string;
  anchor: AlbumCatalogAnchor;
};

export default function AlbumCompleteTrackList(props: Props) {
  const [opened, setOpened] = useState(false);
  if (!opened)
    return (
      <section aria-label="Complete track list">
        <button
          type="button"
          className="button"
          onClick={() => setOpened(true)}
        >
          Show complete track list
        </button>
      </section>
    );
  return <AlbumCatalogBrowser {...props} />;
}

function AlbumCatalogBrowser({ account, release, anchor }: Props) {
  const artist = useReviewedArtistProfile(account, anchor);
  return (
    <section aria-label="Complete track list" aria-busy={artist.pending}>
      <h2>Complete track list</h2>
      <p>
        Choose the release and edition to compare with this album. Nothing is
        saved until you play a catalog track.
      </p>
      {artist.pending && <p role="status">Loading the reviewed artist…</p>}
      {artist.error && (
        <>
          <p role="alert">{artist.error}</p>
          <button
            type="button"
            className="button"
            disabled={artist.pending}
            onClick={() => void artist.retry()}
          >
            Try again
          </button>
        </>
      )}
      {artist.ready && (
        <ArtistDiscography
          account={account}
          recording={anchor.recording_id}
          association={artist.ready.association}
          profile={artist.ready.profile}
          browseOnMount
          release={release}
        />
      )}
    </section>
  );
}
