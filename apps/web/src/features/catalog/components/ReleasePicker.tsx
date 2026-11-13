import type { Schema } from "../../../shared/schema";
import CatalogCover from "../../artists/components/CatalogCover";
import MediaCard from "../../../shared/ui/MediaCard";
import { groupSubtitle } from "../model/releaseMatch";
import MatchDialog from "./MatchDialog";

export default function ReleasePicker({
  title,
  groups,
  pending,
  canLoadMore,
  more,
  pick,
  dismiss,
}: {
  title: string;
  groups: Schema<"CatalogReleaseGroup">[];
  pending: boolean;
  canLoadMore: boolean;
  more: () => void;
  pick: (group: Schema<"CatalogReleaseGroup">) => void;
  dismiss: () => void;
}) {
  return (
    <MatchDialog
      title={`Which release is ${title}?`}
      busy={pending}
      onClose={dismiss}
    >
      <ul className="release-grid" aria-label="Releases to choose from">
        {groups.map((group) => (
          <li key={group.id} className="release-card">
            <MediaCard
              title={group.title}
              subtitle={groupSubtitle(group)}
              heading="h3"
              cover={
                <CatalogCover
                  group={group.id}
                  edition={null}
                  title={group.title}
                  artist={group.artist_credit?.[0]?.credited_name ?? null}
                />
              }
              openLabel={`Choose ${group.title}`}
              onOpen={() => pick(group)}
            />
          </li>
        ))}
      </ul>
      {pending && <p role="status">Loading more releases…</p>}
      <div className="match-actions">
        {canLoadMore && (
          <button
            type="button"
            className="text-action"
            disabled={pending}
            onClick={more}
          >
            Load more releases
          </button>
        )}
        <button type="button" className="text-action" onClick={dismiss}>
          Just show my tracks
        </button>
      </div>
    </MatchDialog>
  );
}
