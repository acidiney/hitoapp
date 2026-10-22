import { Link, router } from "@inertiajs/react";
import type { RecordingDetailsPageProps } from "../shared/types";
import Artwork from "../features/artwork/components/Artwork";
import ArtworkStatus from "../features/artwork/components/ArtworkStatus";
import MetadataEditor from "../components/MetadataEditor";
import MetadataObservations from "../components/MetadataObservations";
import ArtistReview from "../features/artists/components/ArtistReview";
import RecordingCredits from "../components/metadata/RecordingCredits";
import SourceAvailability from "../components/SourceAvailability";
import SourceAttachment from "../components/SourceAttachment";
import SourceChoiceControls from "../components/SourceChoiceControls";
import RecordingDownload from "../features/downloads/components/RecordingDownload";
import { useRecordingDownload } from "../features/downloads/hooks/useRecordingDownload";
import { useSourceChoice } from "../hooks/useSourceChoice";
import RecordingLyrics from "../components/lyrics/RecordingLyrics";
import { useRecordingLyrics } from "../hooks/useRecordingLyrics";

export default function RecordingDetailsPage({
  user,
  detail,
  native_playback,
}: RecordingDetailsPageProps) {
  const download = useRecordingDownload(user.id, detail.recording_id);
  const lyrics = useRecordingLyrics(user.id, detail);
  const choice = useSourceChoice(
    user.id,
    detail.recording_id,
    download.invalidateRead,
  );
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
      {source?.origin === "provider_response" && source.attribution && (
        <section aria-label="Selected metadata credits">
          <h2>Selected metadata credits</h2>
          <RecordingCredits attribution={source.attribution} />
        </section>
      )}
      <MetadataEditor
        key={`editor/${user.id}/${target.id}`}
        account={user.id}
        {...target}
        metadata={detail}
      />
      <ArtistReview
        key={JSON.stringify([
          "artist-review",
          user.id,
          detail.recording_id,
          source?.id,
          detail.artist.operation_id,
        ])}
        account={user.id}
        detail={detail}
      />
      <RecordingLyrics
        providers={lyrics.providers}
        available={lyrics.available}
        pending={lyrics.pending}
        result={lyrics.result}
        error={lyrics.error}
        refresh={lyrics.refresh}
        onLoad={() => void lyrics.load()}
        onCancel={lyrics.cancel}
        onRefreshDetails={
          lyrics.providers.engaged
            ? lyrics.refreshProviders
            : () => router.reload()
        }
      />
      <RecordingDownload
        {...download}
        local={native_playback}
        eligibilityPending={choice.eligibilityPending}
      />
      <SourceChoiceControls {...choice} />
      {source?.origin === "provider_response" && (
        <SourceAttachment
          key={`attachment/${user.id}/${detail.recording_id}`}
          account={user.id}
          detail={detail}
          onAttached={choice.invalidateRead}
        />
      )}
      <SourceAvailability
        key={`availability/${user.id}/${detail.recording_id}`}
        account={user.id}
        choiceRevision={choice.choice?.revision}
        eligibilityPending={choice.eligibilityPending}
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
