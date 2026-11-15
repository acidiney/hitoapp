import type { Schema } from "../../../shared/schema";
import { regionName } from "../../../shared/regionName";
import CatalogCover from "../../artists/components/CatalogCover";
import { suggestedSubtitle, groupSubtitle } from "../model/releaseMatch";
import MatchDialog from "./MatchDialog";

function editionName(edition: Schema<"CatalogEditionSummary">) {
  return edition.disambiguation
    ? `${edition.title} (${edition.disambiguation})`
    : edition.title;
}

function editionDetail(edition: Schema<"CatalogEditionSummary">) {
  return (
    [edition.release_date, edition.country ? regionName(edition.country) : null]
      .filter(Boolean)
      .join(" · ") || "Release date and country not provided"
  );
}

export default function EditionChoicePanel({
  title,
  group,
  suggested,
  editions,
  selected,
  pending,
  select,
  use,
  different,
  dismiss,
  close,
  more,
}: {
  title: string;
  group: Schema<"CatalogReleaseGroup">;
  suggested: boolean;
  editions: Schema<"CatalogEditionSummary">[];
  selected: string;
  pending: boolean;
  select: (id: string) => void;
  use: () => void;
  different: () => void;
  dismiss: () => void;
  close: () => void;
  more: (() => void) | null;
}) {
  return (
    <MatchDialog title={title} busy={pending} onClose={close}>
      <div className="match-release">
        <span className="match-release-cover">
          <CatalogCover
            group={group.id}
            edition={null}
            title={group.title}
            artist={group.artist_credit?.[0]?.credited_name ?? null}
          />
        </span>
        <span className="match-release-text">
          <span>{group.title}</span>
          <span className="match-note">
            {suggested ? suggestedSubtitle(group) : groupSubtitle(group)}
          </span>
        </span>
      </div>
      <fieldset className="match-options">
        <legend>
          Which track list? Editions that differ only by country or audio
          quality count as one.
        </legend>
        {editions.map((edition) => (
          <label
            key={edition.id}
            className="match-option"
            data-selected={edition.id === selected || undefined}
          >
            <input
              type="radio"
              name="album-edition"
              checked={edition.id === selected}
              disabled={pending}
              onChange={() => select(edition.id)}
            />
            <span className="match-option-text">
              <span>{editionName(edition)}</span>
              <span className="match-note">{editionDetail(edition)}</span>
            </span>
          </label>
        ))}
      </fieldset>
      {pending && <p role="status">Loading editions…</p>}
      <div className="match-actions">
        <button
          type="button"
          className="pill-action"
          data-primary=""
          disabled={pending || selected === ""}
          onClick={use}
        >
          Use this
        </button>
        {more && (
          <button type="button" className="text-action" onClick={more}>
            Load more editions
          </button>
        )}
        <button type="button" className="text-action" onClick={different}>
          It's a different release
        </button>
        <button type="button" className="text-action" onClick={dismiss}>
          Just show my tracks
        </button>
      </div>
    </MatchDialog>
  );
}
