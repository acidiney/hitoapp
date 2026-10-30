import type { Schema } from "../../../shared/schema";
import { useState } from "react";

import type { ReleaseType, TypePaging } from "../hooks/useDiscographyTypes";
import { releaseAvailability } from "../model/releaseAvailability";
import DiscographyRelease from "./DiscographyRelease";
import Button from "../../../shared/ui/Button";

const SECTION_PAGE = 10;

const sections = [
  { key: "album", title: "Albums", noun: "albums", label: "Albums" },
  { key: "ep", title: "EPs", noun: "EPs", label: "EPs" },
  { key: "single", title: "Singles", noun: "singles", label: "Singles" },
  {
    key: "other",
    title: "Other releases",
    noun: "other releases",
    label: "Other",
  },
] as const;

type SectionKey = (typeof sections)[number]["key"];

function sectionOf(group: Schema<"CatalogReleaseGroup">): SectionKey {
  return group.kind === "album" ||
    group.kind === "ep" ||
    group.kind === "single"
    ? group.kind
    : "other";
}

export type TypeControls = {
  catalogComplete: boolean;
  unsupported: boolean;
  pending: ReleaseType | null;
  types: Partial<Record<ReleaseType, TypePaging>>;
  more: (type: ReleaseType) => void;
};

export default function DiscographySections({
  account,
  recording,
  association,
  profile,
  groups,
  library,
  release,
  onAlbumEdition,
  paging,
}: {
  account: string;
  recording: string;
  association: Schema<"ArtistAssociation">;
  profile: Schema<"CatalogArtistProfile">;
  groups: Schema<"CatalogReleaseGroup">[];
  library: Record<string, Schema<"LibraryAlbum">[]>;
  release?: string;
  onAlbumEdition?: (edition: Schema<"SavedAlbumEdition"> | null) => void;
  paging?: TypeControls;
}) {
  const [only, setOnly] = useState<SectionKey | null>(null);
  const [shown, setShown] = useState<Partial<Record<SectionKey, number>>>({});
  const [missingOnly, setMissingOnly] = useState(false);
  const wanted = missingOnly
    ? groups.filter(
        (group) =>
          !(
            library[group.id] !== undefined &&
            releaseAvailability(library[group.id]).complete
          ),
      )
    : groups;
  const present = sections.filter((section) =>
    groups.some((group) => sectionOf(group) === section.key),
  );
  const canPageEmpty = (key: SectionKey) => {
    const state = paging?.types[key];
    return (
      !!paging &&
      !paging.unsupported &&
      !paging.catalogComplete &&
      !(state && state.pages > 0 && state.continuation === null)
    );
  };
  return (
    <>
      <div className="discography-controls">
        {present.length > 1 && (
          <div className="segmented" role="group" aria-label="Release types">
            <button
              type="button"
              aria-pressed={only === null}
              onClick={() => setOnly(null)}
            >
              {`All · ${groups.length}`}
            </button>
            {present.map((section) => (
              <button
                key={section.key}
                type="button"
                aria-pressed={only === section.key}
                onClick={() =>
                  setOnly(only === section.key ? null : section.key)
                }
              >
                {`${section.label} · ${groups.filter((group) => sectionOf(group) === section.key).length}`}
              </button>
            ))}
          </div>
        )}
        <button
          type="button"
          className="chip-toggle"
          aria-pressed={missingOnly}
          onClick={() => setMissingOnly(!missingOnly)}
        >
          Only what I'm missing
        </button>
      </div>
      {paging?.unsupported && (
        <p role="status">
          This provider cannot page one release type at a time. Showing the
          releases loaded so far; use Load more releases for the rest.
        </p>
      )}
      {missingOnly && wanted.length === 0 && groups.length > 0 && (
        <p role="status" aria-label="Nothing missing">
          Every release here is already in your library.
        </p>
      )}
      {sections
        .filter(
          (section) =>
            (present.includes(section) || canPageEmpty(section.key)) &&
            (only === null || only === section.key),
        )
        .map((section) => {
          const members = wanted.filter(
            (group) => sectionOf(group) === section.key,
          );
          const empty = !present.includes(section);
          if (members.length === 0 && !empty) return null;
          const state = paging?.types[section.key];
          const visible = state?.started
            ? Math.max(shown[section.key] ?? SECTION_PAGE, members.length)
            : (shown[section.key] ?? SECTION_PAGE);
          const hidden = members.length > visible;
          const providerHasMore =
            !!paging &&
            !paging.unsupported &&
            !paging.catalogComplete &&
            !(state && state.pages > 0 && state.continuation === null);
          return (
            <section key={section.key} aria-label={section.title}>
              <h4>{`${section.title} (${members.length})`}</h4>
              <ul>
                {members.slice(0, visible).map((group) => (
                  <DiscographyRelease
                    key={group.id}
                    account={account}
                    recording={recording}
                    association={association}
                    profile={profile}
                    group={group}
                    library={library[group.id]}
                    heading="h5"
                    release={release}
                    onAlbumEdition={onAlbumEdition}
                  />
                ))}
              </ul>
              {paging?.pending === section.key && (
                <p role="status">{`Loading more ${section.noun}…`}</p>
              )}
              {state?.error && (
                <>
                  <p role="alert">{state.error}</p>
                  <Button
                    type="button"
                    disabled={paging?.pending != null}
                    onClick={() => paging?.more(section.key)}
                  >
                    {state.restartRequired
                      ? `Restart ${section.noun}`
                      : "Try again"}
                  </Button>
                </>
              )}
              {!state?.error && (hidden || providerHasMore) && (
                <Button
                  type="button"
                  disabled={!hidden && paging?.pending != null}
                  onClick={() =>
                    hidden
                      ? setShown({
                          ...shown,
                          [section.key]: visible + SECTION_PAGE,
                        })
                      : paging?.more(section.key)
                  }
                >
                  {empty ? `Show ${section.noun}` : `Show more ${section.noun}`}
                </Button>
              )}
            </section>
          );
        })}
    </>
  );
}
