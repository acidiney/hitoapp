import Button from "../../../shared/ui/Button";
import {
  useProviderArtistMatching,
  type ProviderMatchState,
} from "../hooks/useProviderArtistMatching";

function summary(state: ProviderMatchState) {
  switch (state.status) {
    case "disabled":
      return "Turn on the built-in MusicBrainz provider in metadata settings to use this.";
    case "failed":
      return "The search could not finish. Try again later.";
    case "running":
      return "Searching MusicBrainz…";
    case "finished": {
      const { matched, uncertain, unavailable } = state.counts;
      const parts = `${matched} matched, ${uncertain} uncertain, ${unavailable} unavailable.`;
      return state.complete ? parts : `${parts} Run again to continue.`;
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
  const busy = state.status === "running";
  const result = summary(state);
  return (
    <section aria-labelledby="provider-matching-title">
      <h2 className="discography-heading" id="provider-matching-title">
        Match with MusicBrainz
      </h2>
      <p className="muted" id="provider-matching-help">
        Sends ISRC codes, provider artist links and the artist names of tracks
        it cannot place otherwise from your library to MusicBrainz.
      </p>
      <Button
        aria-describedby="provider-matching-help"
        disabled={
          state.status === "checking" || state.status === "disabled" || busy
        }
        onClick={() => void run()}
      >
        Find matches on MusicBrainz
      </Button>
      <p className="muted" role="status" aria-live="polite">
        {result}
      </p>
    </section>
  );
}
