import type { Schema } from "../../../shared/schema";
import { useMemo } from "react";
import { useArtistDiscography } from "../hooks/useArtistDiscography";
import { useDiscographyLibrary } from "../hooks/useDiscographyLibrary";
import { useDiscographyTypes } from "../hooks/useDiscographyTypes";

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
  const typed = useDiscographyTypes(
    account,
    recording,
    association,
    profile,
    showLibrary,
  );
  const groups = useMemo(() => {
    const known = new Set(workflow.groups.map((group) => group.id));
    return [
      ...workflow.groups,
      ...typed.groups.filter((group) => !known.has(group.id)),
    ];
  }, [workflow.groups, typed.groups]);
  const library = useDiscographyLibrary(
    account,
    recording,
    association,
    groups,
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
          groups={groups}
          library={library.albums}
          release={release}
          onAlbumEdition={onAlbumEdition}
          paging={{
            catalogComplete:
              workflow.pages > 0 &&
              workflow.continuation === null &&
              !workflow.limited &&
              !workflow.restartRequired &&
              !workflow.error,
            unsupported: typed.unsupported,
            pending: typed.pending,
            types: typed.types,
            more: (type) => void typed.more(type),
          }}
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
        groups.length === 0 &&
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
