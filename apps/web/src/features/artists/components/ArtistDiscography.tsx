import { useArtistDiscography } from "../hooks/useArtistDiscography";
import type {
  ArtistAssociation,
  CatalogArtistProfile,
  SavedAlbumEdition,
} from "../../../shared/types";
import ArtistEditions from "./ArtistEditions";

const kinds = {
  album: "Album",
  ep: "EP",
  single: "Single",
  broadcast: "Broadcast",
  other: "Other",
};

export default function ArtistDiscography({
  account,
  recording,
  association,
  profile,
  browseOnMount = false,
  release,
  onAlbumEdition,
}: {
  account: string;
  recording: string;
  association: ArtistAssociation;
  profile: CatalogArtistProfile;
  browseOnMount?: boolean;
  release?: string;
  onAlbumEdition?: (edition: SavedAlbumEdition | null) => void;
}) {
  const workflow = useArtistDiscography(
    account,
    recording,
    association,
    profile,
    browseOnMount,
  );
  if (!workflow.opened)
    return (
      <button
        type="button"
        className="button"
        onClick={() => void workflow.browse()}
      >
        Browse discography
      </button>
    );
  return (
    <section aria-label="Discography" aria-busy={workflow.pending}>
      <h3>Discography</h3>
      {workflow.pending && (
        <p role="status">
          {workflow.pages > 0
            ? "Loading more releases…"
            : "Loading discography…"}
        </p>
      )}
      {workflow.error && <p role="alert">{workflow.error}</p>}
      <ul aria-label="Discography releases">
        {workflow.groups.map((group) => (
          <li key={group.id}>
            <h4>{group.title}</h4>
            <p>{group.kind ? kinds[group.kind] : "Type not provided"}</p>
            {group.secondary_types.length > 0 && (
              <p>{group.secondary_types.join(" · ")}</p>
            )}
            <p>
              {group.artist_credit?.length
                ? group.artist_credit
                    .map((member) => member.credited_name + member.join_phrase)
                    .join("")
                : "Artist credit not provided"}
            </p>
            <p>{group.first_release_date ?? "Release date not provided"}</p>
            <ArtistEditions
              account={account}
              recording={recording}
              association={association}
              profile={profile}
              group={group}
              release={release}
              onAlbumEdition={onAlbumEdition}
            />
          </li>
        ))}
      </ul>
      {workflow.pages > 0 &&
        workflow.groups.length === 0 &&
        !workflow.pending &&
        !workflow.error && <p>No releases were returned for this artist.</p>}
      {workflow.limited && (
        <p>Showing the first 50 pages of this artist's catalog.</p>
      )}
      {workflow.restartRequired ? (
        <button
          type="button"
          className="button"
          disabled={workflow.pending}
          onClick={() => void workflow.restart()}
        >
          Restart discography
        </button>
      ) : workflow.error ? (
        <button
          type="button"
          className="button"
          disabled={workflow.pending}
          onClick={() => void workflow.retry()}
        >
          Try again
        </button>
      ) : (
        workflow.continuation && (
          <button
            type="button"
            className="button"
            disabled={workflow.pending}
            onClick={() => void workflow.more()}
          >
            Load more releases
          </button>
        )
      )}
    </section>
  );
}
