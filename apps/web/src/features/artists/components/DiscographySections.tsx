import type { Schema } from "../../../shared/schema";
import { useMemo, useState } from "react";

import type { ReleaseType, TypePaging } from "../hooks/useDiscographyTypes";
import DiscographyRelease from "./DiscographyRelease";
import Button from "../../../shared/ui/Button";
import Listbox from "../../../shared/ui/Listbox";

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

const sorts = [
  { value: "newest", label: "Newest first" },
  { value: "oldest", label: "Oldest first" },
  { value: "title", label: "A to Z" },
];

type SectionKey = (typeof sections)[number]["key"];
type SortKey = (typeof sorts)[number]["value"];
type LibraryFilter = "in" | "not" | null;

function sectionOf(group: Schema<"CatalogReleaseGroup">): SectionKey {
  return group.kind === "album" ||
    group.kind === "ep" ||
    group.kind === "single"
    ? group.kind
    : "other";
}

function ordered(groups: Schema<"CatalogReleaseGroup">[], sort: SortKey) {
  const date = (group: Schema<"CatalogReleaseGroup">) =>
    group.first_release_date ?? "";
  return [...groups].sort((a, b) => {
    if (sort === "title") return a.title.localeCompare(b.title);
    if (!date(a) || !date(b)) return date(a) ? -1 : date(b) ? 1 : 0;
    return sort === "oldest"
      ? date(a).localeCompare(date(b))
      : date(b).localeCompare(date(a));
  });
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
  albumHref,
  paging,
}: {
  albumHref?: (id: string) => string;
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
  const [filter, setFilter] = useState<LibraryFilter>(null);
  const [sort, setSort] = useState<SortKey>("newest");
  const owned = (group: Schema<"CatalogReleaseGroup">) =>
    (library[group.id]?.length ?? 0) > 0;
  const wanted = useMemo(
    () =>
      ordered(
        filter === null
          ? groups
          : groups.filter((group) =>
              filter === "in" ? owned(group) : !owned(group),
            ),
        sort,
      ),
    [groups, library, filter, sort],
  );
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
  const unloaded = sections.filter(
    (section) => !present.includes(section) && canPageEmpty(section.key),
  );
  const chips = [...present, ...unloaded];
  const toggleFilter = (next: Exclude<LibraryFilter, null>) =>
    setFilter(filter === next ? null : next);
  return (
    <>
      <div className="discography-controls">
        <div className="chip-row" role="group" aria-label="Release types">
          <button
            type="button"
            className="filter-chip"
            aria-pressed={only === null}
            onClick={() => setOnly(null)}
          >
            {`All ${groups.length}`}
          </button>
          {chips.map((section) => {
            const count = groups.filter(
              (group) => sectionOf(group) === section.key,
            ).length;
            return (
              <button
                key={section.key}
                type="button"
                className="filter-chip"
                aria-pressed={only === section.key}
                onClick={() =>
                  setOnly(only === section.key ? null : section.key)
                }
              >
                {count > 0 ? `${section.label} ${count}` : section.label}
              </button>
            );
          })}
          <span className="chip-divider" aria-hidden="true" />
          <button
            type="button"
            className="filter-chip"
            aria-pressed={filter === "in"}
            onClick={() => toggleFilter("in")}
          >
            In my library
          </button>
          <button
            type="button"
            className="filter-chip"
            aria-pressed={filter === "not"}
            onClick={() => toggleFilter("not")}
          >
            Not linked to my library
          </button>
        </div>
        <div className="sort-control discography-sort">
          <Listbox
            label="Sort"
            inlineLabel
            options={sorts}
            value={sort}
            onChange={(value) => setSort(value as SortKey)}
          />
        </div>
      </div>
      {paging?.unsupported && (
        <p role="status" className="detail-section-note">
          This provider cannot page one release type at a time. Showing the
          releases loaded so far; use Load more releases for the rest.
        </p>
      )}
      {filter !== null && wanted.length === 0 && groups.length > 0 && (
        <p
          role="status"
          aria-label="Nothing to show"
          className="detail-section-note"
        >
          {filter === "in"
            ? "None of these releases is in your library yet."
            : "Every release here is already in your library."}
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
          const total = groups.filter(
            (group) => sectionOf(group) === section.key,
          ).length;
          return (
            <section
              key={section.key}
              className="release-section"
              aria-label={section.title}
            >
              <div
                className={
                  empty
                    ? "release-section-head sr-only"
                    : "release-section-head"
                }
              >
                <h3 aria-label={`${section.title} (${members.length})`}>
                  {section.title}
                </h3>
                {members.length > 0 && (
                  <span className="detail-section-note">
                    {hidden ? `${visible} of ${total}` : total}
                  </span>
                )}
              </div>
              <ul className="release-grid">
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
                    albumHref={albumHref}
                  />
                ))}
              </ul>
              {paging?.pending === section.key && (
                <p
                  role="status"
                  className="detail-section-note"
                >{`Loading more ${section.noun}…`}</p>
              )}
              {state?.error && (
                <div className="text-actions">
                  <p role="alert" className="detail-section-note">
                    {state.error}
                  </p>
                  <Button
                    type="button"
                    disabled={paging?.pending != null}
                    onClick={() => paging?.more(section.key)}
                  >
                    {state.restartRequired
                      ? `Restart ${section.noun}`
                      : "Try again"}
                  </Button>
                </div>
              )}
              {!state?.error && (hidden || providerHasMore) && (
                <div className="text-actions">
                  <button
                    type="button"
                    className="text-action"
                    aria-label={
                      empty
                        ? `Show ${section.noun}`
                        : `Show more ${section.noun}`
                    }
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
                    {empty
                      ? `Show ${section.noun}`
                      : hidden
                        ? `Show ${members.length - visible} more ${section.noun}`
                        : `Show more ${section.noun}`}
                  </button>
                </div>
              )}
            </section>
          );
        })}
    </>
  );
}
