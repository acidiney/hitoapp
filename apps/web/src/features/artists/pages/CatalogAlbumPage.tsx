import { useMemo } from "react";
import { usePage } from "@inertiajs/react";
import type { Schema } from "../../../shared/schema";
import Breadcrumbs from "../../../shared/ui/Breadcrumbs";
import Button from "../../../shared/ui/Button";
import { rememberedReleaseGroup } from "../api/catalogCache";
import CatalogAlbum from "../components/CatalogAlbum";
import { useArtistGroupDiscography } from "../hooks/useArtistGroupDiscography";

function releaseGroupId(url: string) {
  const path = new URL(url, window.location.origin).pathname;
  const segment = path.split("/releases/")[1] ?? "";
  return decodeURIComponent(segment.split("/")[0]);
}

export default function CatalogAlbumPage({
  user,
  playlists,
  detail,
}: Schema<"ArtistIdentityGroupPageProps">) {
  const { group } = detail;
  const { url } = usePage();
  const id = releaseGroupId(url);
  const autoplay =
    new URL(url, window.location.origin).searchParams.get("play") === "1";
  const workflow = useArtistGroupDiscography(user.id, group);
  const ready = workflow.ready;
  const release = useMemo<Schema<"CatalogReleaseGroup">>(
    () =>
      (ready && rememberedReleaseGroup(user.id, ready.association.id, id)) || {
        id,
        title: "",
        secondary_types: [],
      },
    [user.id, ready, id],
  );
  const artistHref = `/artist-groups/${encodeURIComponent(group.id)}`;
  return (
    <>
      {ready ? (
        <CatalogAlbum
          key={`${user.id}/${group.id}/${id}`}
          context={{
            account: user.id,
            recording: group.name.recording_id,
            association: ready.association,
            profile: ready.profile,
            group: release,
          }}
          heading="h1"
          withLibrary
          playOnReady={autoplay}
          playlists={playlists}
          crumbs={
            <Breadcrumbs
              crumbs={[
                { label: "Library", href: "/library" },
                { label: "Artists", href: "/artists" },
                { label: group.name.value, href: artistHref },
                { label: release.title || "Release" },
              ]}
            />
          }
        />
      ) : (
        <section aria-label="Album" aria-busy={workflow.pending}>
          <Breadcrumbs
            crumbs={[
              { label: "Library", href: "/library" },
              { label: "Artists", href: "/artists" },
              { label: group.name.value, href: artistHref },
            ]}
          />
          {workflow.pending && <p role="status">Loading album…</p>}
          {workflow.error && !workflow.pending && (
            <p role="alert">
              {workflow.error}{" "}
              <Button type="button" onClick={() => void workflow.browse()}>
                Try again
              </Button>
            </p>
          )}
        </section>
      )}
    </>
  );
}
