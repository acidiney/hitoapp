import type { Schema } from "../../../shared/schema";
import AvailabilityLabel, {
  type Availability,
} from "../../../shared/ui/AvailabilityLabel";
import { useAlbumLibraryLevel } from "../hooks/useAlbumLibraryLevel";

const levels: Record<
  Schema<"LibraryLevel">,
  { word: string; state: Availability }
> = {
  in_library: { word: "In library", state: "library" },
  partly: { word: "Partly in library", state: "available" },
  linked: { word: "Linked", state: "catalog" },
};

export default function AlbumLevelCheck({
  account,
  release,
  releaseGroup,
}: {
  account: string;
  release: string;
  releaseGroup: string;
}) {
  const check = useAlbumLibraryLevel(account, release, releaseGroup);
  if (check.level?.status === "compared") {
    const { word, state } = levels[check.level.level];
    return (
      <AvailabilityLabel
        state={state}
        label={`${word} · ${check.level.matched_tracks} of ${check.level.edition_tracks} tracks`}
      />
    );
  }
  return (
    <>
      {check.pending && <p role="status">Comparing with the saved edition…</p>}
      {check.error && <p role="alert">{check.error}</p>}
      {!check.pending && (check.error === "" || check.retryable) && (
        <button
          type="button"
          className="text-link"
          onClick={() => void check.compare()}
        >
          {check.error ? "Try again" : "Compare with the saved edition"}
        </button>
      )}
    </>
  );
}
