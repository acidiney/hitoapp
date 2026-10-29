import { useArtistDiscography } from "../hooks/useArtistDiscography";
import { useDiscographyLibrary } from "../hooks/useDiscographyLibrary";
import type {
  ArtistAssociation,
  CatalogArtistProfile,
  SavedAlbumEdition,
} from "../../../shared/types";
import DiscographyRelease from "./DiscographyRelease";
import DiscographySections from "./DiscographySections";

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
  association: ArtistAssociation;
  profile: CatalogArtistProfile;
  browseOnMount?: boolean;
  showLibrary?: boolean;
  release?: string;
  onAlbumEdition?: (edition: SavedAlbumEdition | null) => void;
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
      <button
        type="button"
        className="button"
        onClick={() => void workflow.browse()}
      >
        Browse discography
      </button>
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
          <button type="button" className="button" onClick={library.retry}>
            Check library again
          </button>
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
        <button
          type="button"
          className="button"
          disabled={workflow.pending}
          onClick={() => void workflow.restart()}
        >
          Restart discography
        </button>
      ) : workflow.error ? (
        <button
          type="button"
          className="button"
          disabled={workflow.pending}
          onClick={() => void workflow.retry()}
        >
          Try again
        </button>
      ) : (
        workflow.continuation && (
          <button
            type="button"
            className="button"
            disabled={workflow.pending}
            onClick={() => void workflow.more()}
          >
            Load more releases
          </button>
        )
      )}
    </section>
  );
}
