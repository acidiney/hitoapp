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
      <h2 id="local-file-source">Playback source</h2>
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
      <p>Hito plays the file from your music folder.</p>
    </section>
  );
}

export default function TrackSources({
  account,
  detail,
  choice,
  localFile,
  download,
  nativePlayback,
}: {
  account: string;
  detail: Schema<"RecordingMetadata">;
  choice: ReturnType<typeof useSourceChoice>;
  localFile: boolean;
  download: ReturnType<typeof useRecordingDownload>;
  nativePlayback: boolean;
}) {
  const source = detail.selected_observation;
  const sources = choice.choice?.sources ?? [];
  const hasSources = sources.length > 0;
  const active =
    sources.find((entry) => entry.selected && entry.source.ready) ??
    sources.find((entry) => entry.source.ready);
  const localOnly = active ? isLocalSource(active.source) : localFile;
  const showCopy = !localOnly || download.download !== null;
  return (
    <div className="track-sources">
      {source?.origin === "provider_response" && !hasSources && (
        <SourceAttachment
          key={`attachment/${account}/${detail.recording_id}`}
          account={account}
          detail={detail}
          prominent
          onAttached={choice.invalidateRead}
        />
      )}
      {localFile && !hasSources && <LocalFileSource />}
      <SourceChoiceControls {...choice} />
      {source?.origin === "provider_response" && hasSources && (
        <SourceAttachment
          key={`attachment/${account}/${detail.recording_id}`}
          account={account}
          detail={detail}
          prominent={false}
          onAttached={choice.invalidateRead}
        />
      )}
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
