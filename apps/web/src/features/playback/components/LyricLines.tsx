import { useRef } from "react";
import { ArrowUp } from "lucide-react";
import type { Schema } from "../../../shared/schema";
import { usePlayer } from "./PlaybackProvider";
import { useLyricFollow } from "../hooks/useLyricFollow";

type Line = Schema<"TimedLyricLine">;

export function SyncedLines({ lines }: { lines: Line[] }) {
  const player = usePlayer();
  const scroller = useRef<HTMLDivElement>(null);
  const follow = useLyricFollow(lines, scroller);
  return (
    <>
      <div
        ref={scroller}
        className="np-lines"
        data-synced=""
        aria-label="Lyric lines"
        role="group"
        tabIndex={0}
        onWheel={follow.detach}
        onTouchMove={follow.detach}
        onKeyDown={(event) => {
          if (
            ["ArrowUp", "ArrowDown", "PageUp", "PageDown"].includes(event.key)
          )
            follow.detach();
        }}
      >
        {lines.map((line, index) => (
          <button
            key={index}
            type="button"
            data-line={index}
            data-state={
              index === follow.current
                ? "current"
                : index < follow.current
                  ? "past"
                  : "next"
            }
            aria-current={index === follow.current ? "true" : undefined}
            onClick={() => player.seek(line.start_ms / 1000)}
          >
            {line.text}
          </button>
        ))}
      </div>
      {follow.detached && (
        <div className="np-rejoin">
          <button type="button" onClick={follow.rejoin}>
            <ArrowUp size={14} strokeWidth={3} aria-hidden="true" />
            Back to current line
          </button>
          <span>
            Appears only after you scroll by hand, which pauses following.
          </span>
        </div>
      )}
    </>
  );
}

export function UnplayedLines({ lines }: { lines: Line[] }) {
  return (
    <div
      className="np-lines"
      data-synced=""
      aria-label="Lyric lines"
      role="group"
      tabIndex={0}
    >
      {lines.map((line, index) => (
        <button
          key={index}
          type="button"
          data-line={index}
          data-state="next"
          disabled
        >
          {line.text}
        </button>
      ))}
    </div>
  );
}

export function PlainLines({ lines }: { lines: { text: string }[] }) {
  return (
    <div
      className="np-lines"
      aria-label="Lyric lines"
      role="group"
      tabIndex={0}
    >
      {lines.map((line, index) =>
        line.text === "" ? (
          <span key={index} data-blank="" aria-hidden="true" />
        ) : (
          <span key={index}>{line.text}</span>
        ),
      )}
    </div>
  );
}
