import { usePage } from "@inertiajs/react";
import type { Schema } from "../../../shared/schema";
import { rememberedReleaseGroup } from "../api/catalogCache";
import CatalogAlbum from "../components/CatalogAlbum";
import { useCatalogAlbumAnchor } from "../hooks/useArtistGroupDiscography";
import { useCatalogReleaseGroup } from "../hooks/useCatalogReleaseGroup";

export default function CatalogAlbumPage({
  user,
  playlists,
  detail,
  release_group_id: releaseGroupId,
}: Schema<"CatalogAlbumPageProps">) {
  const { group } = detail;
  const { url } = usePage();
  const autoplay =
    new URL(url, window.location.origin).searchParams.get("play") === "1";
  const anchor = useCatalogAlbumAnchor(user.id, group);
  const association = anchor.ready?.association ?? null;
  const fresh = useCatalogReleaseGroup(
    user.id,
    group.name.recording_id,
    association,
    releaseGroupId,
  );
  const summary =
    fresh ??
    (association
      ? rememberedReleaseGroup(user.id, association.id, releaseGroupId)
      : null);
  const artistHref = `/artist-groups/${encodeURIComponent(group.id)}`;
  return (
    <>
      {association ? (
        <CatalogAlbum
          key={`${user.id}/${group.id}/${releaseGroupId}`}
          context={{
            account: user.id,
            recording: group.name.recording_id,
            association,
            providerArtistId: group.provider_artist_id,
            releaseGroupId,
          }}
          group={summary}
          heading="h1"
          withLibrary
          artistHref={artistHref}
          playOnReady={autoplay}
          playlists={playlists}
        />
      ) : (
        <section
          className="detail-page"
          aria-label="Album"
          aria-busy={anchor.pending}
        >
          <div className="detail-body">
            {anchor.pending && (
              <div className="state-card">
                <span className="state-card-title" role="status">
                  Loading album…
                </span>
              </div>
            )}
            {anchor.error && !anchor.pending && (
              <div className="state-card">
                <span className="state-card-title" role="alert">
                  {anchor.error}
                </span>
                <button
                  type="button"
                  className="text-action"
                  onClick={() => void anchor.browse()}
                >
                  Try again
                </button>
              </div>
            )}
          </div>
        </section>
      )}
    </>
  );
}
