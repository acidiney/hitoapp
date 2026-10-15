import { Link } from "@inertiajs/react";
import type { RecordingDetailsPageProps } from "../types";
import Artwork from "../components/Artwork";
import RecordingTitleEditor from "../components/RecordingTitleEditor";
import MetadataObservations from "../components/MetadataObservations";

export default function RecordingDetailsPage({
  user,
  detail,
}: RecordingDetailsPageProps) {
  const source = detail.selected_observation;
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
          <p>{source?.artist ?? "Source metadata unavailable"}</p>
        </div>
      </section>
      <RecordingTitleEditor
        key={`${user.id}/${detail.recording_id}/${detail.title.operation_id ?? "inherited"}`}
        account={user.id}
        recording={detail.recording_id}
        title={detail.title}
      />
      <MetadataObservations
        key={`${user.id}/${detail.recording_id}/${source?.id ?? "none"}/${detail.title.operation_id ?? "inherited"}`}
        account={user.id}
        recording={detail.recording_id}
        initial={detail.observations}
        selected={source?.id}
      />
    </div>
  );
}
