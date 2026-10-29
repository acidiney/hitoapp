import { useState } from "react";
import type {
  ArtistAssociation,
  CatalogArtistProfile,
  CatalogReleaseGroup,
  LibraryAlbum,
  SavedAlbumEdition,
} from "../../../shared/types";
import DiscographyRelease from "./DiscographyRelease";

const SECTION_PAGE = 10;

const sections = [
  { key: "album", title: "Albums", noun: "albums", only: "Albums only" },
  { key: "ep", title: "EPs", noun: "EPs", only: "EPs only" },
  { key: "single", title: "Singles", noun: "singles", only: "Singles only" },
  {
    key: "other",
    title: "Other releases",
    noun: "other releases",
    only: "Other releases only",
  },
] as const;

type SectionKey = (typeof sections)[number]["key"];

function sectionOf(group: CatalogReleaseGroup): SectionKey {
  return group.kind === "album" ||
    group.kind === "ep" ||
    group.kind === "single"
    ? group.kind
    : "other";
}

export default function DiscographySections({
  account,
  recording,
  association,
  profile,
  groups,
  library,
  release,
  onAlbumEdition,
}: {
  account: string;
  recording: string;
  association: ArtistAssociation;
  profile: CatalogArtistProfile;
  groups: CatalogReleaseGroup[];
  library: Record<string, LibraryAlbum[]>;
  release?: string;
  onAlbumEdition?: (edition: SavedAlbumEdition | null) => void;
}) {
  const [only, setOnly] = useState<SectionKey | null>(null);
  const [shown, setShown] = useState<Partial<Record<SectionKey, number>>>({});
  const present = sections.filter((section) =>
    groups.some((group) => sectionOf(group) === section.key),
  );
  return (
    <>
      {present.length > 1 && (
        <div role="group" aria-label="Release types">
          {present.map((section) => (
            <button
              key={section.key}
              type="button"
              className="button"
              aria-pressed={only === section.key}
              onClick={() => setOnly(only === section.key ? null : section.key)}
            >
              {section.only}
            </button>
          ))}
        </div>
      )}
      {present
        .filter((section) => only === null || only === section.key)
        .map((section) => {
          const members = groups.filter(
            (group) => sectionOf(group) === section.key,
          );
          const visible = shown[section.key] ?? SECTION_PAGE;
          return (
            <section key={section.key} aria-label={section.title}>
              <h4>{`${section.title} (${members.length})`}</h4>
              <ul aria-label={section.title}>
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
              {members.length > visible && (
                <button
                  type="button"
                  className="button"
                  onClick={() =>
                    setShown({
                      ...shown,
                      [section.key]: visible + SECTION_PAGE,
                    })
                  }
                >
                  {`Show more ${section.noun}`}
                </button>
              )}
            </section>
          );
        })}
    </>
  );
}
