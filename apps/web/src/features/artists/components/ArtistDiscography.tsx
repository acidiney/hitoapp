import type { Schema } from "../../../shared/schema";
import { useMemo } from "react";
import { useArtistDiscography } from "../hooks/useArtistDiscography";
import { useDiscographyLibrary } from "../hooks/useDiscographyLibrary";
import { useDiscographyTypes } from "../hooks/useDiscographyTypes";

import DiscographyRelease from "./DiscographyRelease";
import DiscographySections from "./DiscographySections";
import Button from "../../../shared/ui/Button";
import DiscographyState from "./DiscographyState";
import OwnAlbums from "./OwnAlbums";
import UnmatchedAlbums from "./UnmatchedAlbums";
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
  own = [],
}: {
  own?: Schema<"Release">[];
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
  const catalogName =
    association.provider.kind === "builtin_musicbrainz"
      ? "MusicBrainz"
      : "your catalog service";
  const loadedAll =
    workflow.pages > 0 &&
    workflow.continuation === null &&
    !workflow.limited &&
    !workflow.restartRequired &&
    !workflow.error;
  const showingOwn = showLibrary && groups.length === 0;
  const matched = new Set(
    Object.values(library.albums).flatMap((albums) =>
      albums.map((album) => album.release_id),
    ),
  );
  const unmatched = own.filter((item) => !matched.has(item.id));
  const failed =
    showLibrary &&
    groups.length === 0 &&
    !!workflow.error &&
    !workflow.retrying &&
    !workflow.pending;
  const empty =
    showLibrary &&
    workflow.pages > 0 &&
    groups.length === 0 &&
    !workflow.pending &&
    !workflow.error;
  return (
    <>
      {showingOwn && <OwnAlbums releases={own} />}
      <section
        className="detail-section discography"
        aria-label="Discography"
        aria-busy={workflow.pending}
      >
        {showLibrary ? (
          <div className="detail-section-head">
            <h2>{groups.length > 0 ? "Discography" : "Full discography"}</h2>
            {workflow.pending ? (
              <span
                className="source-status detail-section-note"
                data-status="checking"
                role="status"
              >
                <span className="source-status-dot" aria-hidden="true" />
                {workflow.pages > 0
                  ? "Loading more releases…"
                  : `Loading from ${catalogName}…`}
              </span>
            ) : (
              groups.length > 0 && (
                <span className="detail-section-note">
                  {`from ${catalogName} · ${loadedAll ? `all ${groups.length} releases loaded` : `${groups.length} releases loaded so far`}`}
                </span>
              )
            )}
          </div>
        ) : (
          <h3>Discography</h3>
        )}
        {!showLibrary && workflow.pending && (
          <p role="status">
            {workflow.pages > 0
              ? "Loading more releases…"
              : "Loading discography…"}
          </p>
        )}
        {showLibrary && workflow.pending && groups.length === 0 && (
          <div className="release-skeleton" aria-hidden="true">
            {[70, 55, 80, 45].map((width) => (
              <span key={width}>
                <span className="release-skeleton-cover" />
                <span
                  className="release-skeleton-line"
                  style={{ width: `${width}%` }}
                />
              </span>
            ))}
          </div>
        )}
        {refreshing && !workflow.pending && (
          <p role="status" className="detail-section-note">
            Checking for updates…
          </p>
        )}
        {workflow.error && !workflow.retrying && !showLibrary && (
          <p role="alert">{workflow.error}</p>
        )}
        {failed && (
          <DiscographyState
            title="Couldn't load the discography"
            text={`${catalogName} couldn't be loaded. Your own albums are still listed.`}
            role="alert"
            hidden={workflow.error}
          >
            <button
              type="button"
              className="text-action"
              disabled={workflow.pending}
              onClick={() =>
                void (workflow.restartRequired
                  ? workflow.restart()
                  : workflow.retry())
              }
            >
              Try again
            </button>
          </DiscographyState>
        )}
        {empty && (
          <DiscographyState
            title="No releases listed for this artist"
            text={`${catalogName} lists nothing for this artist. Your own albums are still listed.`}
          />
        )}
        {showLibrary && library.error && (
          <p role="status" className="detail-section-note">
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
                catalogComplete: loadedAll,
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
        {!showLibrary &&
          workflow.pages > 0 &&
          groups.length === 0 &&
          !workflow.pending &&
          !workflow.error && <p>No releases were returned for this artist.</p>}
        {workflow.limited && (
          <p>Showing the first 50 pages of this artist's catalog.</p>
        )}
        {showLibrary &&
        groups.length === 0 ? null : workflow.restartRequired ? (
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
      {showLibrary && <UnmatchedAlbums releases={unmatched} />}
    </>
  );
}
