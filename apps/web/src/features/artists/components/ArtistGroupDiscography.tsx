import type { Schema } from "../../../shared/schema";
import { useArtistGroupDiscography } from "../hooks/useArtistGroupDiscography";

import ArtistDiscography from "./ArtistDiscography";
import DiscographyState from "./DiscographyState";
import OwnAlbums from "./OwnAlbums";

export default function ArtistGroupDiscography({
  account,
  group,
  own,
}: {
  account: string;
  group: Schema<"ArtistIdentityGroupDetail">["group"];
  own: Schema<"Release">[];
}) {
  const workflow = useArtistGroupDiscography(account, group);
  if (workflow.ready)
    return (
      <ArtistDiscography
        account={account}
        recording={group.name.recording_id}
        association={workflow.ready.association}
        profile={workflow.ready.profile}
        browseOnMount
        showLibrary
        own={own}
        albumHref={(release) =>
          `/artist-groups/${encodeURIComponent(group.id)}/releases/${encodeURIComponent(release)}`
        }
      />
    );
  return (
    <>
      <OwnAlbums releases={own} />
      <section
        className="detail-section"
        aria-label="Full discography"
        aria-busy={workflow.pending}
      >
        <div className="detail-section-head">
          <h2>Full discography</h2>
          {workflow.pending && (
            <span
              className="source-status detail-section-note"
              data-status="checking"
              role="status"
            >
              <span className="source-status-dot" aria-hidden="true" />
              Loading chosen artist…
            </span>
          )}
        </div>
        {workflow.error && !workflow.retrying && !workflow.pending && (
          <DiscographyState
            title="Couldn't load the discography"
            text="Your own albums are still listed."
            role="alert"
            hidden={workflow.error}
          >
            <button
              type="button"
              className="text-action"
              disabled={workflow.pending}
              onClick={() => void workflow.browse()}
            >
              Try again
            </button>
          </DiscographyState>
        )}
      </section>
    </>
  );
}
