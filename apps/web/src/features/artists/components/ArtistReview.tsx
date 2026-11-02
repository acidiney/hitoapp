import { regionName } from "../../../shared/regionName";
import type { Schema } from "../../../shared/schema";
import {
  useEffect,
  useId,
  useRef,
  useState,
  type FormEvent,
  type ReactNode,
} from "react";
import { Link } from "@inertiajs/react";
import { useArtistAssociationChanges } from "../hooks/useArtistAssociationChanges";
import { useArtistIdentityChoice } from "../hooks/useArtistIdentityChoice";
import { useArtistReview } from "../hooks/useArtistReview";
import { useSharedUnlink } from "../hooks/useSharedUnlink";
import ArtistDiscography from "./ArtistDiscography";
import ArtistPhoto from "./ArtistPhoto";
import CandidateReleases from "./CandidateReleases";
import ArtistIdentityChoice from "./ArtistIdentityChoice";
import SavedArtistReviews from "./SavedArtistReviews";
import SharedUnlink from "./SharedUnlink";
import { providerName } from "../model/associationEvidence";

import Button from "../../../shared/ui/Button";

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
  openSearch = false,
}: {
  account: string;
  detail: Schema<"RecordingMetadata">;
  openSearch?: boolean;
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
  const changes = useArtistAssociationChanges(
    account,
    detail,
    workflow.loaded,
    async () => {
      await Promise.all([workflow.refreshAssociations(), identity.refresh()]);
    },
  );
  const sharing = useSharedUnlink(
    account,
    detail.recording_id,
    changes.state,
    changes.reload,
    () => void workflow.refreshAssociations(),
  );
  const [panel, setPanel] = useState(openSearch);
  const requested = useRef(false);
  useEffect(() => {
    if (!openSearch || requested.current) return;
    requested.current = true;
    void workflow.open();
  });
  const panelId = useId();
  const panelHeading = useRef<HTMLHeadingElement>(null);
  const panelOpen = panel && workflow.loaded;
  useEffect(() => {
    if (panelOpen) panelHeading.current?.focus();
  }, [panelOpen]);
  const locked =
    workflow.pending === "confirm" || workflow.pending === "profile";
  function search(event: FormEvent) {
    event.preventDefault();
    void workflow.search();
  }
  return (
    <section
      className="rd-card"
      aria-label="Artist information"
      aria-busy={workflow.pending !== null}
    >
      <h2>Artist information</h2>
      <p>
        Hito matches this track's artist for you. Change it only if the match is
        wrong.
      </p>
      <ArtistSummary
        credit={detail.artist.value}
        loaded={workflow.loaded}
        choice={identity.choice}
        name={identity.name}
        association={
          identity.choice?.state.status === "selected"
            ? workflow.associations.find(
                (item) =>
                  identity.choice?.state.status === "selected" &&
                  item.id === identity.choice.state.association_id,
              )
            : undefined
        }
        action={
          <Button
            type="button"
            aria-expanded={panelOpen}
            aria-controls={panelId}
            disabled={workflow.pending !== null && !panel}
            onClick={() => {
              if (!workflow.loaded) void workflow.open();
              setPanel(workflow.loaded ? !panel : true);
            }}
          >
            Change artist
          </Button>
        }
      />
      {workflow.pending === "load" && (
        <p role="status" className="muted">
          Loading artist information…
        </p>
      )}
      {workflow.error && <p role="alert">{workflow.error}</p>}
      {panelOpen && (
        <div className="rd-manual" id={panelId}>
          <h3 ref={panelHeading} tabIndex={-1}>
            Choose the artist
          </h3>
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
            <form className="rd-search" onSubmit={search}>
              <fieldset disabled={locked}>
                <label className="rd-field">
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
                <label className="rd-field rd-grow">
                  Search artists
                  <input
                    type="search"
                    value={workflow.query}
                    maxLength={512}
                    onChange={(event) => workflow.setQuery(event.target.value)}
                  />
                </label>
                <Button
                  type="submit"
                  disabled={
                    workflow.pending !== null ||
                    !workflow.provider?.available ||
                    !workflow.query.trim()
                  }
                >
                  {workflow.pending === "search"
                    ? "Searching artists…"
                    : "Search artists"}
                </Button>
              </fieldset>
            </form>
          )}
          {workflow.results && (
            <ArtistCandidates
              account={account}
              recording={detail.recording_id}
              context={workflow.results.context}
              candidates={workflow.results.page.items}
              photos={workflow.provider?.value.kind === "builtin_musicbrainz"}
              disabled={workflow.pending !== null}
              onReview={workflow.previewCandidate}
            />
          )}
          {workflow.results?.page.items.length === 0 && (
            <p>No artists found. Try another name.</p>
          )}
          {workflow.results?.page.next_cursor && (
            <p>
              More artists are available. Narrow your search to find a closer
              match.
            </p>
          )}
          {workflow.preview && (
            <section className="rd-panel" aria-label="Review artist">
              <h3>Review artist</h3>
              <h4>Saved track credit</h4>
              <p>{detail.artist.value}</p>
              <h4>Provider profile</h4>
              <ArtistFacts profile={workflow.preview.profile} />
              <p>
                Save this artist review for this track. Your saved details and
                playlists are kept.
              </p>
              <Button
                variant="primary"
                type="button"
                disabled={workflow.pending !== null}
                onClick={() => void workflow.confirm()}
              >
                {workflow.pending === "confirm"
                  ? "Saving artist review…"
                  : "Use this artist"}
              </Button>
            </section>
          )}
          {workflow.saved && <p role="status">Artist review saved.</p>}
          <SavedArtistReviews
            associations={workflow.associations}
            disabled={workflow.pending !== null || !workflow.enabled}
            ready={changes.ready}
            canUndo={changes.canUndo}
            retrying={changes.retrying}
            error={changes.error}
            onView={(association) => void workflow.loadProfile(association)}
            onUnlink={(association) => void changes.unlink(association)}
            onUndo={() => void changes.undo()}
            onRetry={() => void changes.retry()}
          />
          <SharedUnlink state={changes.state} sharing={sharing} />
          {workflow.pending === "profile" && (
            <p role="status">Loading reviewed artist profile…</p>
          )}
          {workflow.profile?.status === "available" && (
            <section className="rd-panel" aria-label="Reviewed artist profile">
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
              This saved artist profile is unavailable for this track. Review
              the artist again or check your metadata providers.
            </p>
          )}
        </div>
      )}
    </section>
  );
}

function ArtistSummary({
  credit,
  loaded,
  choice,
  name,
  association,
  action,
}: {
  credit: string;
  loaded: boolean;
  choice: Schema<"RecordingArtistIdentityChoice"> | null;
  name: string | null;
  association: Schema<"ArtistAssociation"> | undefined;
  action: ReactNode;
}) {
  const state = choice?.state;
  const chosen = state?.status === "selected";
  const provenance = chosen
    ? association && association.evidence !== "user_confirmed"
      ? `Matched automatically from ${providerName(association.provider)}`
      : "Chosen by you"
    : loaded
      ? "Not matched yet — Hito looks this up automatically"
      : null;
  return (
    <div className="rd-summary">
      <div className="rd-row-copy">
        <strong>Artist: {chosen && name ? name : credit}</strong>
        {provenance && <p>{provenance}</p>}
      </div>
      <div className="rd-row-actions">
        {chosen && (
          <Link
            className="text-link"
            href={`/artist-groups/${encodeURIComponent(state.group_id)}`}
          >
            Open artist
          </Link>
        )}
        {action}
      </div>
    </div>
  );
}

function candidateFacts(candidate: Schema<"ArtistSearchCandidate">) {
  return [
    candidate.kind ? kinds[candidate.kind] : "Type not provided",
    candidate.country ? regionName(candidate.country) : null,
    candidate.begin_year ? `${candidate.begin_year}–` : null,
  ]
    .filter(Boolean)
    .join(" · ");
}

function ArtistCandidates({
  account,
  recording,
  context,
  candidates,
  photos,
  disabled,
  onReview,
}: {
  account: string;
  recording: string;
  context: Schema<"ArtistReviewContext">;
  candidates: Schema<"ArtistSearchCandidate">[];
  photos: boolean;
  disabled: boolean;
  onReview(candidate: Schema<"ArtistSearchCandidate">): Promise<void>;
}) {
  return (
    <ul className="rd-rows" aria-label="Artist search results">
      {candidates.map((candidate) => (
        <li className="rd-row rd-candidate" key={candidate.artist.id}>
          <span className="rd-candidate-photo">
            <ArtistPhoto
              artist={candidate.artist.id}
              identity={candidate.artist.id}
              name={candidate.artist.name}
              enabled={photos}
            />
          </span>
          <h3>{candidate.artist.name}</h3>
          <p>{candidateFacts(candidate)}</p>
          {candidate.disambiguation && <p>{candidate.disambiguation}</p>}
          <CandidateReleases
            account={account}
            recording={recording}
            context={context}
            artist={candidate.artist.id}
          />
          <Button
            type="button"
            className="rd-row-action"
            disabled={disabled}
            onClick={() => void onReview(candidate)}
          >
            Review artist
          </Button>
        </li>
      ))}
    </ul>
  );
}

function ArtistFacts({ profile }: { profile: Schema<"CatalogArtistProfile"> }) {
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
