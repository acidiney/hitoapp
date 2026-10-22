import { useArtistGroupDiscography } from "../../hooks/useArtistGroupDiscography";
import type { ArtistIdentityGroupDetail } from "../../shared/types";
import ArtistDiscography from "./ArtistDiscography";

export default function ArtistGroupDiscography({
  account,
  group,
}: {
  account: string;
  group: ArtistIdentityGroupDetail["group"];
}) {
  const workflow = useArtistGroupDiscography(account, group);
  return (
    <section aria-label="Full discography" aria-busy={workflow.pending}>
      <h2 className="discography-heading">Full discography</h2>
      <p>Browse this chosen artist's catalog beyond your library.</p>
      {workflow.pending && <p role="status">Loading chosen artist…</p>}
      {workflow.error && <p role="alert">{workflow.error}</p>}
      {workflow.ready ? (
        <ArtistDiscography
          account={account}
          recording={group.name.recording_id}
          association={workflow.ready.association}
          profile={workflow.ready.profile}
          browseOnMount
        />
      ) : (
        <button
          type="button"
          className="button"
          disabled={workflow.pending}
          onClick={() => void workflow.browse()}
        >
          {workflow.error ? "Try again" : "Browse full discography"}
        </button>
      )}
    </section>
  );
}
