import type { Schema } from "../../../shared/schema";

type IdentityGroup = Schema<"ArtistIdentityGroupDetail">["group"];

export function sameProvider(
  left: Schema<"MetadataProvider">,
  right: Schema<"MetadataProvider">,
) {
  return (
    left.kind === right.kind &&
    (left.kind === "builtin_musicbrainz" ||
      (right.kind === "http_plugin" &&
        left.installation_id === right.installation_id))
  );
}

export function groupAssociation(
  associations: Schema<"ArtistAssociation">[],
  account: string,
  group: IdentityGroup,
): Schema<"ArtistAssociation"> | null {
  const association = associations.find(
    (item) => item.id === group.name.association_id,
  );
  if (
    !association ||
    association.account_id !== account ||
    association.recording_id !== group.name.recording_id ||
    association.artist_id !== group.name.artist_id ||
    association.provider_artist_id !== group.provider_artist_id ||
    !sameProvider(association.provider, group.provider)
  )
    return null;
  return association;
}

export function associationAuthorizes(scope: {
  account: string;
  recording: string;
  association: Schema<"ArtistAssociation">;
  providerArtistId: string;
}) {
  return (
    scope.association.account_id === scope.account &&
    scope.association.recording_id === scope.recording &&
    scope.association.provider_artist_id === scope.providerArtistId
  );
}
