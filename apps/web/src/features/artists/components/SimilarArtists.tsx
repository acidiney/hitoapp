import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";
import { useSimilarArtists } from "../hooks/useSimilarArtists";

import Button from "../../../shared/ui/Button";

const NOTES: Record<
  Exclude<
    Schema<"SimilarArtistsAvailability">["status"],
    "available" | "unavailable"
  >,
  string
> = {
  unsupported:
    "None of your chosen metadata providers offers similar artists. Choose a provider that offers similar artists, then try again.",
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
  const workflow = useSimilarArtists(account, group);
  if (group.provider.kind !== "builtin_musicbrainz") return null;
  const { result } = workflow;
  return (
    <section
      aria-labelledby="similar-artists-title"
      aria-busy={workflow.pending}
    >
      <h2 className="discography-heading" id="similar-artists-title">
        Similar artists
      </h2>
      {!result && !workflow.pending && !workflow.error && (
        <p>
          Ask your chosen metadata providers for similar artists. Only this
          artist's catalog identity is sent, never your listening history.
        </p>
      )}
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
            {NOTES[result.status]}{" "}
            {result.status === "unsupported" && (
              <Link className="text-link" href="/settings?section=metadata">
                Manage metadata providers
              </Link>
            )}
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
      {!workflow.pending && !(result && !workflow.error) && (
        <Button type="button" onClick={() => void workflow.load()}>
          {workflow.error ? "Try again" : "Show similar artists"}
        </Button>
      )}
    </section>
  );
}
