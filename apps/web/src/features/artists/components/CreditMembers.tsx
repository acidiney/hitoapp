import type { Schema } from "../../../shared/schema";
import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type RefObject,
} from "react";
import { useCreditMemberChoice } from "../hooks/useCreditMemberChoice";
import ArtistFacts from "./ArtistFacts";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";

type Member = Schema<"CreditedArtistView"> & { artist_id: string };

const ROLES: Record<Schema<"CreditRole">, string> = {
  main: "Main",
  featured: "Featured",
};

export default function CreditMembers({
  account,
  detail,
}: {
  account: string;
  detail: Schema<"RecordingMetadata">;
}) {
  const members = detail.credited_artists.filter(
    (member): member is Member => member.artist_id !== null,
  );
  const [expanded, setExpanded] = useState(false);
  const [opened, setOpened] = useState(false);
  if (members.length === 0) return null;
  return (
    <section className="rd-card track-credit-members">
      <h2>
        <button
          type="button"
          className="track-disclosure"
          aria-expanded={expanded}
          aria-controls="credited-artists"
          onClick={() => {
            setExpanded(!expanded);
            setOpened(true);
          }}
        >
          Credited artists ({members.length})
        </button>
      </h2>
      <div
        id="credited-artists"
        role="region"
        aria-label="Credited artists"
        hidden={!expanded}
      >
        {opened && (
          <ul className="rd-rows">
            {members.map((member) => (
              <CreditMemberRow
                key={JSON.stringify([
                  account,
                  detail.recording_id,
                  member.artist_id,
                  member.name,
                  member.role,
                  detail.selected_observation?.id,
                ])}
                account={account}
                detail={detail}
                member={member}
              />
            ))}
          </ul>
        )}
      </div>
    </section>
  );
}

function CreditMemberRow({
  account,
  detail,
  member,
}: {
  account: string;
  detail: Schema<"RecordingMetadata">;
  member: Member;
}) {
  const workflow = useCreditMemberChoice(account, detail, member);
  const [open, setOpen] = useState(false);
  const opener = useRef<HTMLElement | null>(null);
  function close() {
    workflow.dismiss();
    setOpen(false);
  }
  const latest = useRef(workflow);
  latest.current = workflow;
  useEffect(() => {
    void latest.current.read();
  }, []);
  const state = workflow.choice?.state.status;
  const unreadable = workflow.failed === "read" && !workflow.choice;
  const text = unreadable
    ? "Could not load"
    : state === "selected"
      ? workflow.name
        ? `Chosen: ${workflow.name}`
        : "Chosen"
      : state === "needs_review"
        ? "Needs review"
        : state === "unselected"
          ? "Not chosen yet"
          : "Loading…";
  return (
    <li
      className="rd-row"
      aria-label={member.name}
      aria-busy={workflow.pending !== null}
    >
      <div className="rd-row-copy">
        <strong>{member.name}</strong>
        <span className="track-chip" data-tone="neutral">
          {ROLES[member.role]}
        </span>
        <p>{text}</p>
        {workflow.unresolved && (
          <p>Hito could not confirm the last change to this credit.</p>
        )}
        {!open && workflow.error && <p role="alert">{workflow.error}</p>}
      </div>
      <div className="rd-row-actions">
        {unreadable && (
          <Button
            type="button"
            disabled={workflow.pending !== null}
            onClick={() => void workflow.read()}
          >
            Try again
          </Button>
        )}
        {workflow.unresolved && (
          <Button
            type="button"
            disabled={workflow.pending !== null}
            onClick={() => void workflow.checkAgain()}
          >
            Check again
          </Button>
        )}
        {state && state !== "unselected" && (
          <Button
            type="button"
            disabled={workflow.pending !== null || workflow.unresolved}
            onClick={() => void workflow.reset()}
          >
            Clear choice
          </Button>
        )}
        <Button
          type="button"
          disabled={!workflow.choice || workflow.unresolved}
          onClick={(event) => {
            opener.current = event.currentTarget;
            setOpen(true);
          }}
        >
          Choose artist
        </Button>
      </div>
      {open && (
        <CreditMemberDialog
          member={member}
          workflow={workflow}
          opener={opener}
          onClose={close}
        />
      )}
    </li>
  );
}

function CreditMemberDialog({
  member,
  workflow,
  opener,
  onClose,
}: {
  member: Member;
  workflow: ReturnType<typeof useCreditMemberChoice>;
  opener: RefObject<HTMLElement | null>;
  onClose(): void;
}) {
  const { loaded, pending, failed } = workflow;
  const latest = useRef(workflow);
  latest.current = workflow;
  useEffect(() => {
    void latest.current.refresh();
  }, []);
  useEffect(() => {
    if (!loaded && pending === null && failed !== "load")
      void latest.current.open();
  }, [loaded, pending, failed]);
  const busy = pending === "choose";
  const preview = workflow.preview?.profile;
  function search(event: FormEvent) {
    event.preventDefault();
    void workflow.search();
  }
  return (
    <Dialog
      wide
      title={`Choose the artist for ${member.name}`}
      description="Pick which artist this credit means. Your other credits on this track are not changed."
      busy={busy}
      returnFocus={opener}
      onClose={onClose}
    >
      {workflow.pending === "load" && (
        <p role="status" className="muted">
          Loading artist information…
        </p>
      )}
      {workflow.loaded && !workflow.enabled && (
        <p>
          Metadata providers are disabled.{" "}
          <a href="/settings?section=metadata">Manage metadata providers</a>
        </p>
      )}
      {workflow.loaded && workflow.enabled && (
        <form className="rd-search" onSubmit={search}>
          <fieldset disabled={busy}>
            <label className="rd-field">
              Metadata provider
              <select
                value={workflow.selected}
                onChange={(event) => workflow.select(event.target.value)}
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
        <ul className="rd-rows" aria-label="Artist search results">
          {workflow.results.page.items.map((candidate) => (
            <li className="rd-row rd-candidate" key={candidate.artist.id}>
              <h3>{candidate.artist.name}</h3>
              {candidate.disambiguation && <p>{candidate.disambiguation}</p>}
              <Button
                type="button"
                className="rd-row-action"
                disabled={workflow.pending !== null}
                onClick={() => void workflow.previewCandidate(candidate)}
              >
                Preview artist
              </Button>
            </li>
          ))}
        </ul>
      )}
      {workflow.results?.page.items.length === 0 && (
        <p>No artists found. Try another name.</p>
      )}
      {workflow.results?.page.next_cursor && (
        <p>
          More artists are available. Narrow your search to find a closer match.
        </p>
      )}
      {preview && (
        <section className="rd-panel" aria-label="Chosen candidate">
          <ArtistFacts profile={preview} />
        </section>
      )}
      {workflow.error && <p role="alert">{workflow.error}</p>}
      {workflow.unresolved && (
        <Button
          type="button"
          disabled={workflow.pending !== null}
          onClick={() =>
            void workflow
              .checkAgain()
              .then((kind) => kind === "choose" && onClose())
          }
        >
          Check again
        </Button>
      )}
      {failed === "load" && (
        <Button type="button" onClick={() => void workflow.open()}>
          Try again
        </Button>
      )}
      <footer className="review-actions">
        <Button disabled={busy} onClick={onClose}>
          Cancel
        </Button>
        <Button
          variant="primary"
          disabled={
            !preview || workflow.pending !== null || workflow.unresolved
          }
          onClick={() =>
            void workflow.choose().then((done) => done && onClose())
          }
        >
          {busy ? "Saving artist…" : "Use this artist"}
        </Button>
      </footer>
    </Dialog>
  );
}
