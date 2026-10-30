import type { Schema } from "../../../shared/schema";
import { useState } from "react";

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
  association: Schema<"ArtistAssociation">;
  profile: Schema<"CatalogArtistProfile">;
  groups: Schema<"CatalogReleaseGroup">[];
  library: Record<string, Schema<"LibraryAlbum">[]>;
  release?: string;
  onAlbumEdition?: (edition: Schema<"SavedAlbumEdition"> | null) => void;
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
      {missingOnly && wanted.length === 0 && groups.length > 0 && (
        <p role="status" aria-label="Nothing missing">
          Every release here is already in your library.
        </p>
      )}
      {present
        .filter((section) => only === null || only === section.key)
        .map((section) => {
          const members = wanted.filter(
            (group) => sectionOf(group) === section.key,
          );
          if (members.length === 0) return null;
          const visible = shown[section.key] ?? SECTION_PAGE;
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
              {members.length > visible && (
                <Button
                  type="button"
                  onClick={() =>
                    setShown({
                      ...shown,
                      [section.key]: visible + SECTION_PAGE,
                    })
                  }
                >
                  {`Show more ${section.noun}`}
                </Button>
              )}
            </section>
          );
        })}
    </>
  );
}
