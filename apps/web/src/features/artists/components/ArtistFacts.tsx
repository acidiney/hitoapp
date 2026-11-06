import type { Schema } from "../../../shared/schema";

export const artistKinds = {
  person: "Person",
  group: "Group",
  orchestra: "Orchestra",
  choir: "Choir",
  character: "Character",
  other: "Other",
};

export default function ArtistFacts({
  profile,
}: {
  profile: Schema<"CatalogArtistProfile">;
}) {
  return (
    <>
      <h3 className="review-title">{profile.artist.name}</h3>
      <p>{profile.kind ? artistKinds[profile.kind] : "Type not provided"}</p>
      {profile.disambiguation && <p>{profile.disambiguation}</p>}
      {profile.sort_name && profile.sort_name !== profile.artist.name && (
        <p>Sort name: {profile.sort_name}</p>
      )}
    </>
  );
}
