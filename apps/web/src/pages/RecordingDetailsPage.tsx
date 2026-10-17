import { Link } from "@inertiajs/react";
import type { RecordingDetailsPageProps } from "../types";
import Artwork from "../components/Artwork";
import ArtworkStatus from "../components/ArtworkStatus";
import MetadataEditor from "../components/MetadataEditor";
import MetadataObservations from "../components/MetadataObservations";
import SourceAvailability from "../components/SourceAvailability";
import SourceAttachment from "../components/SourceAttachment";
import RecordingDownload from "../components/downloads/RecordingDownload";
import { useRecordingDownload } from "../hooks/useRecordingDownload";

export default function RecordingDetailsPage({
  user,
  detail,
  native_playback,
}: RecordingDetailsPageProps) {
  const download = useRecordingDownload(user.id, detail.recording_id);
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
          <ArtworkStatus recording={detail.recording_id} />
        </div>
      </section>
      <MetadataEditor
        key={`editor/${user.id}/${target.id}`}
        account={user.id}
        target={target}
        metadata={detail}
      />
      <RecordingDownload {...download} local={native_playback} />
      {source?.origin === "provider_response" && (
        <SourceAttachment
          key={`attachment/${user.id}/${detail.recording_id}`}
          account={user.id}
          detail={detail}
        />
      )}
      <SourceAvailability
        key={`availability/${user.id}/${detail.recording_id}`}
        account={user.id}
        track={{
          id: detail.recording_id,
          title: detail.title.value,
          artist: detail.artist.value,
          duration_ms: source?.duration_ms ?? 0,
          available: false,
        }}
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
