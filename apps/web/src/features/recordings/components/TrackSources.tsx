import type { Schema } from "../../../shared/schema";
import RecordingDownload from "../../downloads/components/RecordingDownload";
import SourceAttachment from "../../sources/components/SourceAttachment";
import SourceAvailability from "../../sources/components/SourceAvailability";
import SourceChoiceControls from "../../sources/components/SourceChoiceControls";
import { isLocalSource } from "../model/trackStatus";
import type { useRecordingDownload } from "../../downloads/hooks/useRecordingDownload";
import type { useSourceChoice } from "../../sources/hooks/useSourceChoice";

function LocalFileSource() {
  return (
    <section className="rd-card" aria-labelledby="local-file-source">
      <div className="track-block-head">
        <h2 id="local-file-source">Playback source</h2>
        <span className="track-muted">Automatic</span>
      </div>
      <ul className="rd-rows rd-choices">
        <li className="rd-row rd-choice">
          <label>
            <input type="radio" name="local-file-source" checked readOnly />{" "}
            Local file
          </label>
          <span className="rd-choice-origin">Music folder</span>
          <span className="rd-choice-state">
            <span className="track-chip" data-tone="success">
              Ready
            </span>
          </span>
        </li>
      </ul>
      <p className="track-note">
        Automatic picks the best ready source. With one source there is nothing
        to choose.
      </p>
    </section>
  );
}

export default function TrackSources({
  account,
  detail,
  choice,
  localFile,
  active: tabActive,
  download,
  nativePlayback,
}: {
  account: string;
  detail: Schema<"RecordingMetadata">;
  choice: ReturnType<typeof useSourceChoice>;
  localFile: boolean;
  active: boolean;
  download: ReturnType<typeof useRecordingDownload>;
  nativePlayback: boolean;
}) {
  const source = detail.selected_observation;
  const sources = choice.choice?.sources ?? [];
  const hasSources = sources.length > 0;
  const playing =
    sources.find((entry) => entry.selected && entry.source.ready) ??
    sources.find((entry) => entry.source.ready);
  const localOnly = playing ? isLocalSource(playing.source) : localFile;
  const providerTrack = source?.origin === "provider_response";
  const noAudio = playing === undefined && !localFile;
  const hideChoice =
    noAudio &&
    choice.choice !== null &&
    choice.choice.mode === "automatic" &&
    !choice.error &&
    !choice.statusError;
  const attachment = (prominent: boolean) => (
    <SourceAttachment
      key={`attachment/${account}/${detail.recording_id}`}
      account={account}
      detail={detail}
      prominent={prominent}
      expanded={prominent && tabActive}
      defaultQuery={`${detail.title.value} ${detail.artist.value}`.trim()}
      onAttached={choice.invalidateRead}
    />
  );
  const copyHidden =
    localOnly || (noAudio && !choice.unavailable && !choice.checking);
  const showCopy = !copyHidden || download.download !== null;
  return (
    <div className="track-sources">
      {providerTrack && noAudio && attachment(true)}
      {localFile && !hasSources && <LocalFileSource />}
      {!hideChoice && <SourceChoiceControls {...choice} />}
      {providerTrack && !noAudio && attachment(false)}
      {showCopy && (
        <RecordingDownload
          {...download}
          local={nativePlayback}
          eligibilityPending={choice.eligibilityPending}
        />
      )}
      <SourceAvailability
        key={`availability/${account}/${detail.recording_id}`}
        account={account}
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
    </div>
  );
}
