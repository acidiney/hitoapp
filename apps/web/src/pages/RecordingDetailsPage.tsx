import { Link } from "@inertiajs/react";
import type { RecordingDetailsPageProps } from "../types";
import Artwork from "../components/Artwork";
import MetadataEditor from "../components/MetadataEditor";
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
      <MetadataEditor
        key={`${user.id}/${target.id}`}
        account={user.id}
        target={target}
        metadata={detail}
      />
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
