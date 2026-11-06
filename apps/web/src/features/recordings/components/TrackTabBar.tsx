import { useEffect, useRef, type KeyboardEvent } from "react";
import { trackTabs, type TrackTab } from "../model/trackTabs";

const labels: Record<TrackTab, string> = {
  overview: "Overview",
  lyrics: "Lyrics",
  sources: "Sources",
  metadata: "Metadata",
  history: "History",
};

export function tabId(tab: TrackTab) {
  return `track-tab-${tab}`;
}

export function panelId(tab: TrackTab) {
  return `track-panel-${tab}`;
}

function destination(key: string, index: number): number | null {
  const count = trackTabs.length;
  if (key === "ArrowRight" || key === "ArrowDown") return (index + 1) % count;
  if (key === "ArrowLeft" || key === "ArrowUp")
    return (index + count - 1) % count;
  if (key === "Home") return 0;
  if (key === "End") return count - 1;
  return null;
}

export default function TrackTabBar({
  tab,
  badges,
  onSelect,
}: {
  tab: TrackTab;
  badges: Partial<Record<TrackTab, string>>;
  onSelect(tab: TrackTab): void;
}) {
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);
  useEffect(() => {
    buttons.current[trackTabs.indexOf(tab)]?.scrollIntoView?.({
      block: "nearest",
      inline: "nearest",
    });
  }, [tab]);
  function press(event: KeyboardEvent, index: number) {
    const next = destination(event.key, index);
    if (next === null) return;
    event.preventDefault();
    onSelect(trackTabs[next]);
    buttons.current[next]?.focus();
  }
  return (
    <div className="track-tabs-scroll">
      <div className="segmented track-tabs" role="tablist" aria-label="Track">
        {trackTabs.map((entry, index) => (
          <button
            key={entry}
            ref={(element) => {
              buttons.current[index] = element;
            }}
            type="button"
            role="tab"
            id={tabId(entry)}
            aria-selected={entry === tab}
            aria-controls={panelId(entry)}
            tabIndex={entry === tab ? 0 : -1}
            onClick={() => onSelect(entry)}
            onKeyDown={(event) => press(event, index)}
          >
            {labels[entry]}
            {badges[entry] && (
              <span className="track-tab-badge">{badges[entry]}</span>
            )}
          </button>
        ))}
      </div>
    </div>
  );
}
