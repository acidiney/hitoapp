import { Link } from "@inertiajs/react";
import type { AlbumDetailsPageProps } from "../types";
import Artwork from "../components/Artwork";
import MetadataFieldEditor from "../components/MetadataFieldEditor";
import MetadataObservations from "../components/MetadataObservations";

export default function AlbumDetailsPage({
  user,
  detail,
}: AlbumDetailsPageProps) {
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
          <p>{detail.artist.value}</p>
        </div>
      </section>
      <section
        className="metadata-editor"
        aria-labelledby="personal-details-heading"
      >
        <h2 id="personal-details-heading">Your album details</h2>
        <p>
          Changes apply to this album in your account. Track artists and source
          tags stay as recorded.
        </p>
        {(["title", "artist"] as const).map((field) => (
          <MetadataFieldEditor
            key={`${user.id}/${target.id}/${field}/${detail[field].operation_id ?? "inherited"}`}
            account={user.id}
            target={target}
            field={field}
            metadata={detail[field]}
          />
        ))}
      </section>
      <MetadataObservations
        key={`${user.id}/${target.id}/${detail.title.operation_id ?? "inherited"}/${detail.artist.operation_id ?? "inherited"}`}
        account={user.id}
        target={target}
        initial={detail.observations}
      />
    </div>
  );
}
