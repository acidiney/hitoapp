import type { RowPlayState } from "../hooks/useNowPlaying";

export function PlayingBars({ state }: { state: Exclude<RowPlayState, null> }) {
  return (
    <span className="playing-bars" data-state={state} aria-hidden="true">
      <span />
      <span />
      <span />
    </span>
  );
}

export function PlayingStatus({
  state,
}: {
  state: Exclude<RowPlayState, null>;
}) {
  return (
    <span className="sr-only">
      {state === "playing" ? "Now playing" : "Paused"}
    </span>
  );
}
