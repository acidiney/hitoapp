import { useRef, type FormEvent } from "react";
import { Link } from "@inertiajs/react";
import { Search } from "lucide-react";
import Button from "../../../shared/ui/Button";
import Listbox from "../../../shared/ui/Listbox";
import {
  candidateArtist,
  candidateLength,
} from "../../sources/model/candidateFacts";
import type { LyricFailure } from "../model/lyricFailure";
import type { LyricService } from "../model/lyricIdentity";
import type { LyricCandidate } from "../hooks/useLyricSearch";
import LyricReviewDialog, { type LyricReviewProps } from "./LyricReviewDialog";

export type LyricFinderProps = {
  services: LyricService[] | null;
  servicesFailure: LyricFailure | null;
  service: LyricService | null;
  query: string;
  search: {
    pending: boolean;
    candidates: LyricCandidate[] | null;
    failure: LyricFailure | null;
  };
  review: Omit<LyricReviewProps, "opener" | "service" | "candidate"> & {
    candidate: LyricCandidate | null;
  };
  onService: (installationId: string) => void;
  onQuery: (query: string) => void;
  onSearch: () => void;
  onCancelSearch: () => void;
  onReview: (candidate: LyricCandidate) => void;
};

function candidateFacts(candidate: LyricCandidate): string {
  return [
    candidateArtist(candidate.artist),
    candidate.release?.title,
    candidateLength(candidate.duration_ms),
  ]
    .filter(Boolean)
    .join(" · ");
}

export default function LyricFinder(props: LyricFinderProps) {
  const { services, servicesFailure, service, query, search, review } = props;
  const opener = useRef<HTMLElement | null>(null);
  function submit(event: FormEvent) {
    event.preventDefault();
    props.onSearch();
  }
  return (
    <section className="rd-card lyric-finder" aria-labelledby="find-lyrics">
      <h2 id="find-lyrics">Find lyrics</h2>
      <p>
        Search a connected lyrics service, then check the match before saving it
        to this track.
      </p>
      {servicesFailure && <p role="alert">{servicesFailure.message}</p>}
      {services?.length === 0 && (
        <p>
          No lyrics service is connected.{" "}
          <Link href="/settings?section=plugins">Connect a lyrics service</Link>
        </p>
      )}
      {service && services && services.length > 0 && (
        <form
          className="discovery-search lyric-search"
          role="search"
          aria-label="Lyrics search"
          onSubmit={submit}
        >
          {services.length > 1 ? (
            <div className="discovery-service">
              <Listbox
                label="Lyrics service"
                options={services.map((item) => ({
                  value: item.installation_id,
                  label: item.label,
                }))}
                value={service.installation_id}
                onChange={props.onService}
              />
            </div>
          ) : (
            <p className="lyric-service-name">Searching {service.label}</p>
          )}
          <label>
            Search text
            <span className="discovery-query">
              <Search size={18} aria-hidden="true" />
              <input
                type="search"
                value={query}
                maxLength={512}
                placeholder="Track and artist"
                onChange={(event) => props.onQuery(event.target.value)}
              />
            </span>
          </label>
          <div className="discovery-search-actions">
            <Button
              variant="action"
              type="submit"
              disabled={search.pending || query.trim() === ""}
            >
              {search.pending ? "Searching…" : "Search"}
            </Button>
            {search.pending && (
              <Button onClick={props.onCancelSearch}>Cancel</Button>
            )}
          </div>
        </form>
      )}
      {search.failure && <p role="alert">{search.failure.message}</p>}
      {search.candidates && search.candidates.length === 0 && (
        <p role="status">
          No matches. Change the search text and search again.
        </p>
      )}
      {search.candidates && search.candidates.length > 0 && (
        <ul className="rd-rows lyric-candidates" aria-label="Lyric matches">
          {search.candidates.map((candidate, index) => (
            <li className="rd-row lyric-candidate" key={index}>
              <div className="lyric-candidate-copy">
                <h3>{candidate.title}</h3>
                <p>{candidateFacts(candidate)}</p>
              </div>
              <Button
                aria-label={`Review lyrics for ${candidate.title}`}
                disabled={search.pending}
                onClick={(event) => {
                  opener.current = event.currentTarget;
                  props.onReview(candidate);
                }}
              >
                Review lyrics
              </Button>
            </li>
          ))}
        </ul>
      )}
      {service && review.candidate && (
        <LyricReviewDialog
          {...review}
          candidate={review.candidate}
          service={service}
          opener={opener}
        />
      )}
    </section>
  );
}
