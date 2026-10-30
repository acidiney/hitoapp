import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";

import Artwork from "../../artwork/components/Artwork";
import ArtworkStatus from "../../artwork/components/ArtworkStatus";
import MetadataEditor from "../../metadata/components/MetadataEditor";
import MetadataObservations from "../../metadata/components/MetadataObservations";

export default function AlbumDetailsPage({
  user,
  detail,
}: Schema<"AlbumDetailsPageProps">) {
  const target = { kind: "release", id: detail.release_id } as const;
  return (
    <div className="recording-details">
      <Link
        className="text-link"
        href={`/albums/${encodeURIComponent(target.id)}`}
      >
        Back to album
      </Link>
      <section className="collection-hero metadata-hero">
        <Artwork release={target.id} />
        <div>
          <span>Album details</span>
          <h1>{detail.title.value}</h1>
          <p>{detail.artist.value ?? "Album artist not provided"}</p>
        </div>
      </section>
      <ArtworkStatus release={target.id} />
      <MetadataEditor
        key={`${user.id}/${target.id}`}
        account={user.id}
        {...target}
        metadata={detail}
      />
      <MetadataObservations
        key={`${user.id}/${target.id}/${detail.title.operation_id ?? "inherited"}/${detail.artist.operation_id ?? "inherited"}`}
        account={user.id}
        target={target}
        initial={detail.observations}
      />
    </div>
  );
}
