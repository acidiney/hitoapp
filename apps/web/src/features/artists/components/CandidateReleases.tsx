import { useEffect, useRef } from "react";
import type { Schema } from "../../../shared/schema";
import Button from "../../../shared/ui/Button";
import { useCandidateReleases } from "../hooks/useCandidateReleases";

const shownReleases = 3;

export default function CandidateReleases({
  account,
  recording,
  context,
  artist,
}: {
  account: string;
  recording: string;
  context: Schema<"ArtistReviewContext">;
  artist: string;
}) {
  const { releases, load } = useCandidateReleases(
    account,
    recording,
    context,
    artist,
  );
  const container = useRef<HTMLDivElement>(null);
  const idle = releases.phase === "idle";
  const request = useRef(load);
  request.current = load;
  useEffect(() => {
    const element = container.current;
    if (!idle || !element) return;
    if (typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) {
        observer.disconnect();
        void request.current();
      }
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [idle]);
  return (
    <div className="rd-candidate-releases" ref={container}>
      {releases.phase === "loading" && (
        <p aria-busy="true">Loading releases…</p>
      )}
      {releases.phase === "failed" && (
        <>
          <p>{releases.message}</p>
          <Button type="button" onClick={() => void load()}>
            Try again
          </Button>
        </>
      )}
      {releases.phase === "listed" &&
        (releases.titles.length === 0 ? (
          <p>No releases are listed for this artist.</p>
        ) : (
          <ul aria-label="Known releases">
            {releases.titles.slice(0, shownReleases).map((title, position) => (
              <li key={position}>{title}</li>
            ))}
          </ul>
        ))}
    </div>
  );
}
