import type { Schema } from "../../../shared/schema";
import { Link, router } from "@inertiajs/react";

import Artwork from "../../artwork/components/Artwork";
import ArtworkStatus from "../../artwork/components/ArtworkStatus";
import MetadataEditor from "../../metadata/components/MetadataEditor";
import MetadataObservations from "../../metadata/components/MetadataObservations";
import ArtistReview from "../../artists/components/ArtistReview";
import RecordingCredits from "../../metadata/components/RecordingCredits";
import SourceAvailability from "../../sources/components/SourceAvailability";
import SourceAttachment from "../../sources/components/SourceAttachment";
import SourceChoiceControls from "../../sources/components/SourceChoiceControls";
import RecordingDownload from "../../downloads/components/RecordingDownload";
import CreditLine from "../components/CreditLine";
import RecordingCopies from "../components/RecordingCopies";
import { useRecordingSplit } from "../hooks/useRecordingSplit";
import { useRecordingDownload } from "../../downloads/hooks/useRecordingDownload";
import { useSourceChoice } from "../../sources/hooks/useSourceChoice";
import RecordingLyrics from "../../lyrics/components/RecordingLyrics";
import { useRecordingLyrics } from "../../lyrics/hooks/useRecordingLyrics";

export default function RecordingDetailsPage({
  user,
  detail,
  copies,
  native_playback,
}: Schema<"RecordingDetailsPageProps">) {
  const separate = useRecordingSplit(user.id, detail.recording_id, () =>
    router.reload(),
  );
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
      <header className="rd-header">
        <Artwork
          className="rd-cover"
          recording={detail.recording_id}
          fallback={{
            id: detail.recording_id,
            title: detail.title.value,
            artist: detail.artist.value,
            inline: false,
          }}
        />
        <div className="rd-header-copy">
          <span className="rd-kicker">Track details</span>
          <h1>{detail.title.value}</h1>
          <p>
            <CreditLine
              credits={detail.credited_artists}
              fallback={detail.artist.value}
            />
            {source?.album ? ` · ${source.album}` : ""}
          </p>
          <ArtworkStatus recording={detail.recording_id} />
        </div>
      </header>
      <div className="rd-layout">
        <div className="rd-main">
          <RecordingCopies
            recording={detail.recording_id}
            copies={copies.copies}
            splitting={separate.splitting}
            error={separate.error}
            onSplit={separate.split}
          />
          {source?.origin === "provider_response" && source.attribution && (
            <section className="rd-card" aria-label="Selected metadata credits">
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
        </div>
        <aside className="rd-side" aria-label="Playback, download and lyrics">
          <SourceChoiceControls {...choice} />
          <RecordingDownload
            {...download}
            local={native_playback}
            eligibilityPending={choice.eligibilityPending}
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
        </aside>
      </div>
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
