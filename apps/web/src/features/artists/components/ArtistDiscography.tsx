import type { Schema } from "../../../shared/schema";
import { useArtistDiscography } from "../hooks/useArtistDiscography";
import { useDiscographyLibrary } from "../hooks/useDiscographyLibrary";

import DiscographyRelease from "./DiscographyRelease";
import DiscographySections from "./DiscographySections";
import Button from "../../../shared/ui/Button";

export default function ArtistDiscography({
  account,
  recording,
  association,
  profile,
  browseOnMount = false,
  showLibrary = false,
  release,
  onAlbumEdition,
}: {
  account: string;
  recording: string;
  association: Schema<"ArtistAssociation">;
  profile: Schema<"CatalogArtistProfile">;
  browseOnMount?: boolean;
  showLibrary?: boolean;
  release?: string;
  onAlbumEdition?: (edition: Schema<"SavedAlbumEdition"> | null) => void;
}) {
  const workflow = useArtistDiscography(
    account,
    recording,
    association,
    profile,
    browseOnMount,
  );
  const library = useDiscographyLibrary(
    account,
    recording,
    association,
    workflow.groups,
    showLibrary,
  );
  if (!workflow.opened)
    return (
      <Button type="button" onClick={() => void workflow.browse()}>
        Browse discography
      </Button>
    );
  return (
    <section aria-label="Discography" aria-busy={workflow.pending}>
      <h3>Discography</h3>
      {workflow.pending && (
        <p role="status">
          {workflow.pages > 0
            ? "Loading more releases…"
            : "Loading discography…"}
        </p>
      )}
      {workflow.error && <p role="alert">{workflow.error}</p>}
      {showLibrary && library.error && (
        <p role="status">
          {library.error}{" "}
          <Button type="button" onClick={library.retry}>
            Check library again
          </Button>
        </p>
      )}
      {showLibrary ? (
        <DiscographySections
          account={account}
          recording={recording}
          association={association}
          profile={profile}
          groups={workflow.groups}
          library={library.albums}
          release={release}
          onAlbumEdition={onAlbumEdition}
        />
      ) : (
        <ul aria-label="Discography releases">
          {workflow.groups.map((group) => (
            <DiscographyRelease
              key={group.id}
              account={account}
              recording={recording}
              association={association}
              profile={profile}
              group={group}
              release={release}
              onAlbumEdition={onAlbumEdition}
            />
          ))}
        </ul>
      )}
      {workflow.pages > 0 &&
        workflow.groups.length === 0 &&
        !workflow.pending &&
        !workflow.error && <p>No releases were returned for this artist.</p>}
      {workflow.limited && (
        <p>Showing the first 50 pages of this artist's catalog.</p>
      )}
      {workflow.restartRequired ? (
        <Button
          type="button"
          disabled={workflow.pending}
          onClick={() => void workflow.restart()}
        >
          Restart discography
        </Button>
      ) : workflow.error ? (
        <Button
          type="button"
          disabled={workflow.pending}
          onClick={() => void workflow.retry()}
        >
          Try again
        </Button>
      ) : (
        workflow.continuation && (
          <Button
            type="button"
            disabled={workflow.pending}
            onClick={() => void workflow.more()}
          >
            Load more releases
          </Button>
        )
      )}
    </section>
  );
}
