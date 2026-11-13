import type { Schema } from "../../../shared/schema";
import { router } from "@inertiajs/react";
import { dateOnly } from "../../../shared/format";
import ArtistReview from "../../artists/components/ArtistReview";
import CreditMembers from "../../artists/components/CreditMembers";
import { opensArtistSearch } from "../../artists/model/artistSearchLink";
import MetadataDetails from "../../metadata/components/MetadataDetails";
import RecordingCredits from "../../metadata/components/RecordingCredits";
import { useRecordingSplit } from "../hooks/useRecordingSplit";
import RecordingCopies from "./RecordingCopies";

const PROOF: Record<NonNullable<Schema<"RecordingCopy">["proof"]>, string> = {
  musicbrainz_recording: "same recording",
  isrc: "same ISRC",
};

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
  const others = copies.copies.filter(
    (copy) => copy.recording_id !== detail.recording_id,
  );
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
      <CreditMembers
        key={JSON.stringify([
          "credit-members",
          account,
          detail.recording_id,
          source?.id,
          detail.artist.operation_id,
        ])}
        account={account}
        detail={detail}
      />
      <section className="track-block" aria-labelledby="track-origin-heading">
        <h2 id="track-origin-heading">Where these details come from</h2>
        <dl className="track-facts">
          {source && (
            <div className="track-fact">
              <dt>Selected source</dt>
              <dd>
                {source.provider === "local" ? "Local file" : source.provider}
                {source.observed_at !== null && (
                  <span className="track-aside">
                    · inspected {dateOnly(new Date(source.observed_at * 1000))}
                  </span>
                )}
              </dd>
            </div>
          )}
          {source?.origin === "provider_response" && source.attribution && (
            <div
              className="track-fact track-credits"
              role="region"
              aria-label="Selected metadata credits"
            >
              <dt>Credits</dt>
              <dd>
                <RecordingCredits attribution={source.attribution} />
              </dd>
            </div>
          )}
          {detail.musicbrainz_recording_id !== null && (
            <div className="track-fact">
              <dt>MusicBrainz</dt>
              <dd>
                <a
                  className="track-mono track-id"
                  href={`https://musicbrainz.org/recording/${encodeURIComponent(detail.musicbrainz_recording_id)}`}
                  target="_blank"
                  rel="noreferrer noopener"
                >
                  {detail.musicbrainz_recording_id}
                </a>
              </dd>
            </div>
          )}
          {others.length > 0 && (
            <div className="track-fact">
              <dt>Copies</dt>
              <dd>
                {others.length} other {others.length === 1 ? "copy" : "copies"}
                {" · "}
                <a className="text-link" href="#copies">
                  {others[0].album ?? others[0].title}
                </a>
                {others[0].proof && (
                  <span className="track-aside">
                    · {PROOF[others[0].proof]}
                  </span>
                )}
              </dd>
            </div>
          )}
        </dl>
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
