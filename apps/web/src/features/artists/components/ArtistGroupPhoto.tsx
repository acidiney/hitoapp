import type { ReactNode } from "react";
import type { Schema } from "../../../shared/schema";
import ArtistPhoto from "./ArtistPhoto";

export default function ArtistGroupPhoto({
  group,
  children,
}: {
  group: Schema<"ArtistIdentityGroup">;
  children?: ReactNode;
}) {
  const credited = group.credited_photo_artist_id;
  if (credited) {
    return (
      <ArtistPhoto
        key="credited"
        kind="credited-artist"
        artist={credited}
        identity={group.id}
        name={group.name.value}
      >
        {children}
      </ArtistPhoto>
    );
  }
  return (
    <ArtistPhoto
      key="musicbrainz"
      artist={group.provider_artist_id}
      identity={group.id}
      name={group.name.value}
      enabled={group.provider.kind === "builtin_musicbrainz"}
    >
      {children}
    </ArtistPhoto>
  );
}
