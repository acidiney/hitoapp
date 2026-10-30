import { Link } from "@inertiajs/react";
import { relativeTime } from "../../../shared/format";
import { useArtistMatchingStatus } from "../hooks/useArtistMatchingStatus";
import {
  useProviderArtistMatching,
  type ProviderMatchState,
} from "../hooks/useProviderArtistMatching";

function manualNote(state: ProviderMatchState) {
  if (state.status === "failed") return "Matching artists stopped.";
  if (state.status !== "finished" || state.counts.unavailable === 0) return "";
  return "The provider was unavailable.";
}

export default function ArtistMatchingStatus({ account }: { account: string }) {
  const { status, refresh } = useArtistMatchingStatus(account);
  const { state, run } = useProviderArtistMatching(account, refresh);
  if (!status?.enabled) return null;
  const manual = state.status === "running";
  const busy = manual || status.state !== "idle";
  const matched = manual ? state.matched : status.matched_so_far;
  const line = busy
    ? `Matching artists… ${matched} matched`
    : status.last_finished_at === null
      ? "No run yet."
      : `${status.last_matched} matched · last run ${relativeTime(status.last_finished_at)}`;
  const note = busy ? "" : manualNote(state);
  return (
    <p
      className="muted artist-matching-status"
      role="status"
      aria-live="polite"
    >
      {line}
      {note && ` ${note}`}{" "}
      <span>
        Looks up artists on MusicBrainz ·{" "}
        <Link className="text-link" href="/settings?section=privacy">
          Settings
        </Link>
      </span>
      {!busy && (
        <>
          {" "}
          <button
            type="button"
            className="text-link"
            onClick={() => void run()}
          >
            Match again
          </button>
        </>
      )}
    </p>
  );
}
