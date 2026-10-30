import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";
import { useSimilarArtists } from "../hooks/useSimilarArtists";

import Button from "../../../shared/ui/Button";

const NOTES: Record<
  Exclude<
    Schema<"SimilarArtistsAvailability">["status"],
    "available" | "unavailable" | "unsupported"
  >,
  string
> = {
  disabled: "Metadata providers are disabled in your settings.",
  cancelled: "The similar artists request was cancelled.",
};

export default function SimilarArtists({
  account,
  group,
}: {
  account: string;
  group: Schema<"ArtistIdentityGroupDetail">["group"];
}) {
  const supported = group.provider.kind === "builtin_musicbrainz";
  const workflow = useSimilarArtists(account, group, supported);
  if (!supported) return null;
  const { result } = workflow;
  if (result?.status === "unsupported") return null;
  return (
    <section
      aria-labelledby="similar-artists-title"
      aria-busy={workflow.pending}
    >
      <h2 className="discography-heading" id="similar-artists-title">
        Similar artists
      </h2>
      <p className="muted">
        Only this artist's catalog identity is sent to your chosen metadata
        providers, never your listening history.
      </p>
      {workflow.pending && (
        <p role="status" aria-label="Similar artists result">
          Looking for similar artists…
        </p>
      )}
      {workflow.error && <p role="alert">{workflow.error}</p>}
      {result &&
        result.status !== "available" &&
        result.status !== "unavailable" && (
          <p role="status" aria-label="Similar artists result">
            {NOTES[result.status]}
          </p>
        )}
      {result?.status === "available" &&
        (result.artists.length ? (
          <>
            <ul aria-label="Similar artists">
              {result.artists.map((artist) => (
                <li key={artist.provider_artist_id}>
                  {artist.chosen_artist_id ? (
                    <Link
                      className="text-link"
                      href={`/artist-groups/${encodeURIComponent(artist.chosen_artist_id)}`}
                    >
                      {artist.name}
                    </Link>
                  ) : (
                    <>
                      {artist.name}
                      {" · "}
                      <span>Not one of your chosen artists</span>
                    </>
                  )}
                </li>
              ))}
            </ul>
            <p>
              Suggestions from a connected provider. Nothing here is added to
              your library.
            </p>
          </>
        ) : (
          <p role="status" aria-label="Similar artists result">
            Your provider returned no similar artists for this artist.
          </p>
        ))}
      {!workflow.pending && workflow.error && (
        <Button type="button" onClick={() => void workflow.load()}>
          Try again
        </Button>
      )}
    </section>
  );
}
