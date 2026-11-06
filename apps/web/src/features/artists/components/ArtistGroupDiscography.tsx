import type { Schema } from "../../../shared/schema";
import { useArtistGroupDiscography } from "../hooks/useArtistGroupDiscography";

import ArtistDiscography from "./ArtistDiscography";
import Button from "../../../shared/ui/Button";

export default function ArtistGroupDiscography({
  account,
  group,
}: {
  account: string;
  group: Schema<"ArtistIdentityGroupDetail">["group"];
}) {
  const workflow = useArtistGroupDiscography(account, group);
  return (
    <section aria-label="Full discography" aria-busy={workflow.pending}>
      <h2 className="discography-heading">Full discography</h2>
      {workflow.pending && <p role="status">Loading chosen artist…</p>}
      {workflow.error && !workflow.retrying && !workflow.pending && (
        <p role="alert" className="discography-unavailable">
          <span>Discography unavailable right now</span>
          <span className="sr-only">{workflow.error}</span>
          <Button
            type="button"
            disabled={workflow.pending}
            onClick={() => void workflow.browse()}
          >
            Try again
          </Button>
        </p>
      )}
      {workflow.ready && (
        <ArtistDiscography
          account={account}
          recording={group.name.recording_id}
          association={workflow.ready.association}
          profile={workflow.ready.profile}
          browseOnMount
          showLibrary
          albumHref={(release) =>
            `/artist-groups/${encodeURIComponent(group.id)}/releases/${encodeURIComponent(release)}`
          }
        />
      )}
    </section>
  );
}
