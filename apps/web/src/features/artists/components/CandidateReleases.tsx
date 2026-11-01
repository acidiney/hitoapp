import type { Schema } from "../../../shared/schema";
import Button from "../../../shared/ui/Button";
import { useCandidateReleases } from "../hooks/useCandidateReleases";

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
  return (
    <div className="rd-candidate-releases">
      {releases.phase === "idle" && (
        <Button type="button" onClick={() => void load()}>
          Show known releases
        </Button>
      )}
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
            {releases.titles.map((title, position) => (
              <li key={position}>{title}</li>
            ))}
          </ul>
        ))}
    </div>
  );
}
