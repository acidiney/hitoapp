import {
  useProviderArtistMatching,
  type ProviderMatchState,
} from "../hooks/useProviderArtistMatching";

function summary(state: ProviderMatchState) {
  switch (state.status) {
    case "running":
      return `Matching artists… ${state.matched} matched`;
    case "failed":
      return "Matching artists stopped.";
    case "finished": {
      const { matched, unavailable } = state.counts;
      if (state.complete) return `${matched} matched.`;
      return unavailable > 0
        ? `${matched} matched so far. The provider was unavailable.`
        : `${matched} matched so far.`;
    }
    default:
      return "";
  }
}

export default function ProviderArtistMatching({
  account,
}: {
  account: string;
}) {
  const { state, run } = useProviderArtistMatching(account);
  if (state.status === "checking" || state.status === "disabled") return null;
  const text = summary(state);
  return (
    <p className="muted artist-matching-status" role="status" aria-live="polite">
      {text}
      {state.status !== "running" && (
        <>
          {text && " "}
          <button type="button" className="text-link" onClick={() => void run()}>
            Match again
          </button>
        </>
      )}
    </p>
  );
}
