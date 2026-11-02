import type { ReactNode } from "react";
import type { Schema } from "../../../shared/schema";
import ArtistPhoto from "./ArtistPhoto";
import { useCreditedPhotoReread } from "../hooks/useCreditedPhotoReread";

export default function ArtistGroupPhoto({
  account,
  group,
  children,
}: {
  account: string;
  group: Schema<"ArtistIdentityGroup">;
  children?: ReactNode;
}) {
  const { credit: credited, onMusicBrainzFailure } = useCreditedPhotoReread(
    account,
    group,
  );
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
      onImageError={onMusicBrainzFailure}
    >
      {children}
    </ArtistPhoto>
  );
}
