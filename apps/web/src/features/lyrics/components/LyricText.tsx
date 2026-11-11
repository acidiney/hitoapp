import RecordingCredits, {
  canonicalContentUrl,
} from "../../metadata/components/RecordingCredits";
import { time } from "../../../shared/format";
import type { LyricView } from "../model/lyricContent";

function Provider({
  attribution,
}: {
  attribution: NonNullable<LyricView["attribution"]>;
}) {
  const href = canonicalContentUrl(attribution.contentUrl);
  return (
    <p className="lyric-credit">
      Lyrics from{" "}
      {href ? (
        <a
          href={href}
          target="_blank"
          rel="noopener noreferrer"
          referrerPolicy="no-referrer"
        >
          {attribution.providerLabel}
        </a>
      ) : (
        attribution.providerLabel
      )}
    </p>
  );
}

export default function LyricText({ view }: { view: LyricView }) {
  return (
    <div className="track-lyrics-body">
      {view.label && <h3>{view.label}</h3>}
      <ol
        aria-label="Lyric lines"
        data-synced={view.synced}
        data-kind={view.kind}
      >
        {view.lines.map((line, index) =>
          line.text === "" ? (
            <li key={index} data-blank="" aria-hidden="true" />
          ) : (
            <li
              key={index}
              data-time={
                view.synced && line.startMs !== null
                  ? time(line.startMs / 1000)
                  : undefined
              }
            >
              {line.text}
            </li>
          ),
        )}
      </ol>
      {view.attribution && <Provider attribution={view.attribution} />}
      <RecordingCredits attribution={view.legacyAttribution} />
    </div>
  );
}
