import { useId, useRef, useState, type KeyboardEvent } from "react";
import type { Track } from "../model";
import { LyricsPanel, SourcePanel, UpNextPanel } from "./NowPlayingPanels";

const TABS = ["Up next", "Lyrics", "Source"] as const;
type Tab = (typeof TABS)[number];

export default function NowPlayingTabs({
  track,
  openQueue,
  leave,
}: {
  track: Track | null;
  openQueue: () => void;
  leave: () => void;
}) {
  const [active, setActive] = useState<Tab>("Up next");
  const base = useId();
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);
  function press(event: KeyboardEvent, index: number) {
    const target =
      event.key === "ArrowRight"
        ? (index + 1) % TABS.length
        : event.key === "ArrowLeft"
          ? (index + TABS.length - 1) % TABS.length
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? TABS.length - 1
              : null;
    if (target === null) return;
    event.preventDefault();
    setActive(TABS[target]);
    buttons.current[target]?.focus();
  }
  return (
    <>
      <div className="np-tabs" role="tablist" aria-label="Track details">
        {TABS.map((tab, index) => (
          <button
            key={tab}
            ref={(element) => {
              buttons.current[index] = element;
            }}
            type="button"
            role="tab"
            id={`${base}-${index}`}
            aria-selected={tab === active}
            aria-controls={`${base}-panel`}
            tabIndex={tab === active ? 0 : -1}
            onClick={() => setActive(tab)}
            onKeyDown={(event) => press(event, index)}
          >
            {tab}
          </button>
        ))}
      </div>
      <div
        role="tabpanel"
        id={`${base}-panel`}
        aria-labelledby={`${base}-${TABS.indexOf(active)}`}
        className="np-panel"
        tabIndex={active === "Lyrics" && !track ? 0 : undefined}
      >
        {active === "Up next" && <UpNextPanel openQueue={openQueue} />}
        {active === "Lyrics" && <LyricsPanel track={track} leave={leave} />}
        {active === "Source" && <SourcePanel track={track} leave={leave} />}
      </div>
    </>
  );
}
