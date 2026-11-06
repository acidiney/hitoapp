import type { Schema } from "../../../shared/schema";
import { useMemo } from "react";
import { useArtistDiscography } from "../hooks/useArtistDiscography";
import { useDiscographyLibrary } from "../hooks/useDiscographyLibrary";
import { useDiscographyTypes } from "../hooks/useDiscographyTypes";

import DiscographyRelease from "./DiscographyRelease";
import DiscographySections from "./DiscographySections";
import Button from "../../../shared/ui/Button";
import { useCatalogRefreshing } from "../api/catalogCache";

export default function ArtistDiscography({
  account,
  recording,
  association,
  profile,
  browseOnMount = false,
  showLibrary = false,
  release,
  onAlbumEdition,
  albumHref,
}: {
  albumHref?: (id: string) => string;
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
  const refreshing = useCatalogRefreshing();
  if (!workflow.opened)
    return (
      <Button type="button" onClick={() => void workflow.browse()}>
        Browse discography
      </Button>
    );
  return (
    <section aria-label="Discography" aria-busy={workflow.pending}>
      {!showLibrary && <h3>Discography</h3>}
      {workflow.pending && (
        <p role="status">
          {workflow.pages > 0
            ? "Loading more releases…"
            : "Loading discography…"}
        </p>
      )}
      {refreshing && !workflow.pending && (
        <p role="status" className="quiet">
          Checking for updates…
        </p>
      )}
      {workflow.error &&
        !workflow.retrying &&
        !(showLibrary && groups.length === 0) && (
          <p role="alert">{workflow.error}</p>
        )}
      {showLibrary &&
        groups.length === 0 &&
        workflow.error &&
        !workflow.retrying &&
        !workflow.pending && (
          <p role="alert" className="discography-unavailable">
            <span>Discography unavailable right now</span>
            <span className="sr-only">{workflow.error}</span>
            <Button
              type="button"
              disabled={workflow.pending}
              onClick={() =>
                void (workflow.restartRequired
                  ? workflow.restart()
                  : workflow.retry())
              }
            >
              Try again
            </Button>
          </p>
        )}
      {showLibrary && library.error && (
        <p role="status">
          {library.error}{" "}
          <Button type="button" onClick={library.retry}>
            Check library again
          </Button>
        </p>
      )}
      {showLibrary ? (
        (groups.length > 0 || workflow.pages > 0) && (
          <DiscographySections
            account={account}
            recording={recording}
            association={association}
            profile={profile}
            groups={groups}
            library={library.albums}
            release={release}
            onAlbumEdition={onAlbumEdition}
            albumHref={albumHref}
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
        )
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
      {showLibrary &&
      groups.length === 0 &&
      workflow.error ? null : workflow.restartRequired ? (
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
