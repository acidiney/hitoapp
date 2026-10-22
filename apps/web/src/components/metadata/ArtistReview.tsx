import type { FormEvent } from "react";
import { useArtistReview } from "../../hooks/useArtistReview";
import ArtistDiscography from "./ArtistDiscography";
import ArtistIdentityChoice from "./ArtistIdentityChoice";
import { useArtistIdentityChoice } from "../../hooks/useArtistIdentityChoice";
import type {
  ArtistSearchCandidate,
  CatalogArtistProfile,
  RecordingMetadata,
} from "../../shared/types";

const kinds = {
  person: "Person",
  group: "Group",
  orchestra: "Orchestra",
  choir: "Choir",
  character: "Character",
  other: "Other",
};

export default function ArtistReview({
  account,
  detail,
}: {
  account: string;
  detail: RecordingMetadata;
}) {
  const workflow = useArtistReview(account, detail);
  const reviewed =
    workflow.profile?.status === "available" ? workflow.profile : null;
  const association = reviewed
    ? workflow.associations.find((item) => item.id === reviewed.association_id)
    : undefined;
  const identity = useArtistIdentityChoice(
    account,
    detail,
    workflow.loaded,
    association,
  );
  const locked =
    workflow.pending === "confirm" || workflow.pending === "profile";
  function search(event: FormEvent) {
    event.preventDefault();
    void workflow.search();
  }
  return (
    <section
      aria-label="Artist information"
      aria-busy={workflow.pending !== null}
    >
      <h2>Artist information</h2>
      <p>
        Find an artist through your metadata providers and review their profile
        before saving this track's artist review.
      </p>
      {!workflow.loaded && (
        <button
          type="button"
          className="button"
          disabled={workflow.pending !== null}
          onClick={() => void workflow.open()}
        >
          {workflow.pending === "load"
            ? "Loading artist information…"
            : "Find artist information"}
        </button>
      )}
      {workflow.error && <p role="alert">{workflow.error}</p>}
      {workflow.loaded && (
        <ArtistIdentityChoice
          {...identity}
          onChoose={() => void identity.choose()}
          onReset={() => void identity.reset()}
          onRetry={() => void identity.retry()}
          onRefresh={() => void identity.refresh()}
        />
      )}
      {workflow.loaded && !workflow.enabled && (
        <p>
          Metadata providers are disabled.{" "}
          <a href="/settings?section=metadata">Manage metadata providers</a>
        </p>
      )}
      {workflow.loaded && workflow.enabled && (
        <form onSubmit={search}>
          <fieldset disabled={locked}>
            <label>
              Metadata provider
              <select
                value={workflow.selected}
                onChange={(event) => workflow.choose(event.target.value)}
              >
                {workflow.providers.map((provider) => (
                  <option
                    key={provider.key}
                    value={provider.key}
                    disabled={!provider.available}
                  >
                    {provider.label}
                  </option>
                ))}
              </select>
            </label>
            {!workflow.provider?.available && (
              <p>
                The selected metadata provider is unavailable.{" "}
                <a href="/settings?section=metadata">
                  Manage metadata providers
                </a>
              </p>
            )}
            <label>
              Search artists
              <input
                type="search"
                value={workflow.query}
                maxLength={512}
                onChange={(event) => workflow.setQuery(event.target.value)}
              />
            </label>
            <button
              type="submit"
              className="button"
              disabled={
                workflow.pending !== null ||
                !workflow.provider?.available ||
                !workflow.query.trim()
              }
            >
              {workflow.pending === "search"
                ? "Searching artists…"
                : "Search artists"}
            </button>
          </fieldset>
        </form>
      )}
      {workflow.results && (
        <ArtistCandidates
          candidates={workflow.results.page.items}
          disabled={workflow.pending !== null}
          onReview={workflow.previewCandidate}
        />
      )}
      {workflow.results?.page.items.length === 0 && (
        <p>No artists found. Try another name.</p>
      )}
      {workflow.results?.page.next_cursor && (
        <p>
          More artists are available. Narrow your search to find a closer match.
        </p>
      )}
      {workflow.preview && (
        <section aria-label="Review artist">
          <h3>Review artist</h3>
          <h4>Saved track credit</h4>
          <p>{detail.artist.value}</p>
          <h4>Provider profile</h4>
          <ArtistFacts profile={workflow.preview.profile} />
          <p>
            Save this artist review for this track. Your saved details and
            playlists are kept.
          </p>
          <button
            type="button"
            className="button primary"
            disabled={workflow.pending !== null}
            onClick={() => void workflow.confirm()}
          >
            {workflow.pending === "confirm"
              ? "Saving artist review…"
              : "Use this artist"}
          </button>
        </section>
      )}
      {workflow.saved && <p role="status">Artist review saved.</p>}
      {workflow.associations.length > 0 && (
        <section aria-label="Saved artist reviews">
          <h3>Saved artist reviews</h3>
          <ul>
            {workflow.associations.map((association, index) => (
              <li key={association.id}>
                <button
                  type="button"
                  className="button"
                  disabled={workflow.pending !== null || !workflow.enabled}
                  onClick={() => void workflow.loadProfile(association)}
                >
                  View saved artist review {index + 1}
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
      {workflow.pending === "profile" && (
        <p role="status">Loading reviewed artist profile…</p>
      )}
      {workflow.profile?.status === "available" && (
        <section aria-label="Reviewed artist profile">
          <h3>Reviewed artist profile</h3>
          <ArtistFacts profile={workflow.profile.profile} />
          {association && (
            <ArtistDiscography
              account={account}
              recording={detail.recording_id}
              association={association}
              profile={workflow.profile.profile}
            />
          )}
        </section>
      )}
      {workflow.profile && workflow.profile.status !== "available" && (
        <p>
          This saved artist profile is unavailable for this track. Review the
          artist again or check your metadata providers.
        </p>
      )}
    </section>
  );
}

function ArtistCandidates({
  candidates,
  disabled,
  onReview,
}: {
  candidates: ArtistSearchCandidate[];
  disabled: boolean;
  onReview(candidate: ArtistSearchCandidate): Promise<void>;
}) {
  return (
    <ul aria-label="Artist search results">
      {candidates.map((candidate) => (
        <li key={candidate.artist.id}>
          <h3>{candidate.artist.name}</h3>
          <p>{candidate.kind ? kinds[candidate.kind] : "Type not provided"}</p>
          {candidate.disambiguation && <p>{candidate.disambiguation}</p>}
          <button
            type="button"
            className="button"
            disabled={disabled}
            onClick={() => void onReview(candidate)}
          >
            Review artist
          </button>
        </li>
      ))}
    </ul>
  );
}

function ArtistFacts({ profile }: { profile: CatalogArtistProfile }) {
  return (
    <>
      <h3>{profile.artist.name}</h3>
      <p>{profile.kind ? kinds[profile.kind] : "Type not provided"}</p>
      {profile.disambiguation && <p>{profile.disambiguation}</p>}
      {profile.sort_name && profile.sort_name !== profile.artist.name && (
        <p>Sort name: {profile.sort_name}</p>
      )}
    </>
  );
}
