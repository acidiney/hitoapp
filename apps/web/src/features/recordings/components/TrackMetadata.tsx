import type { Schema } from "../../../shared/schema";
import { router } from "@inertiajs/react";
import ArtistReview from "../../artists/components/ArtistReview";
import { opensArtistSearch } from "../../artists/model/artistSearchLink";
import MetadataDetails from "../../metadata/components/MetadataDetails";
import MusicBrainzIds from "../../metadata/components/MusicBrainzIds";
import RecordingCredits from "../../metadata/components/RecordingCredits";
import { useRecordingSplit } from "../hooks/useRecordingSplit";
import RecordingCopies from "./RecordingCopies";

export default function TrackMetadata({
  account,
  detail,
  copies,
}: {
  account: string;
  detail: Schema<"RecordingMetadata">;
  copies: Schema<"RecordingCopies">;
}) {
  const separate = useRecordingSplit(account, detail.recording_id, () =>
    router.reload(),
  );
  const source = detail.selected_observation;
  return (
    <div className="track-metadata">
      <MetadataDetails
        key={`editor/${account}/${detail.recording_id}`}
        account={account}
        detail={detail}
      />
      <ArtistReview
        key={JSON.stringify([
          "artist-review",
          account,
          detail.recording_id,
          source?.id,
          detail.artist.operation_id,
        ])}
        account={account}
        detail={detail}
        openSearch={opensArtistSearch(window.location.search)}
      />
      <section className="track-block" aria-labelledby="track-origin-heading">
        <h2 id="track-origin-heading">Where these details come from</h2>
        {source && (
          <dl className="track-facts">
            <div className="track-fact">
              <dt>Selected source</dt>
              <dd>
                {source.provider === "local" ? "Local file" : source.provider}
                {source.observed_at !== null && (
                  <span className="track-aside">
                    inspected{" "}
                    {new Date(source.observed_at * 1000).toLocaleDateString()}
                  </span>
                )}
              </dd>
            </div>
          </dl>
        )}
        {source?.origin === "provider_response" && source.attribution && (
          <section className="rd-card" aria-label="Selected metadata credits">
            <h3>Selected metadata credits</h3>
            <RecordingCredits attribution={source.attribution} />
          </section>
        )}
        <MusicBrainzIds
          ids={[
            {
              label: "Recording",
              kind: "recording",
              id: detail.musicbrainz_recording_id,
            },
          ]}
        />
      </section>
      <RecordingCopies
        recording={detail.recording_id}
        copies={copies.copies}
        splitting={separate.splitting}
        error={separate.error}
        onSplit={separate.split}
      />
    </div>
  );
}
