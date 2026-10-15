import { Link } from "@inertiajs/react";
import type { RecordingDetailsPageProps } from "../types";
import Artwork from "../components/Artwork";
import MetadataFieldEditor from "../components/MetadataFieldEditor";
import MetadataObservations from "../components/MetadataObservations";

export default function RecordingDetailsPage({
  user,
  detail,
}: RecordingDetailsPageProps) {
  const source = detail.selected_observation;
  const target = { kind: "recording", id: detail.recording_id } as const;
  return (
    <div className="recording-details">
      <Link className="text-link" href="/library">
        Back to library
      </Link>
      <section className="collection-hero metadata-hero">
        <Artwork recording={detail.recording_id} />
        <div>
          <span>Track details</span>
          <h1>{detail.title.value}</h1>
          <p>{detail.artist.value}</p>
        </div>
      </section>
      <section
        className="metadata-editor"
        aria-labelledby="personal-details-heading"
      >
        <h2 id="personal-details-heading">Your track details</h2>
        <p>
          Changes apply to your account across this recording's albums and
          playlists.
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
        key={`${user.id}/${detail.recording_id}/${source?.id ?? "none"}/${detail.title.operation_id ?? "inherited"}/${detail.artist.operation_id ?? "inherited"}`}
        account={user.id}
        target={target}
        initial={detail.observations}
        selected={source?.id}
      />
    </div>
  );
}
