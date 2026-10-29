import { longDuration } from "../../../shared/format";

export type EpisodeState =
  | { kind: "new"; percent: 0; label: string }
  | { kind: "remaining"; percent: number; label: string }
  | { kind: "played"; percent: 100; label: string };

const FINISHED_MARGIN_MS = 1500;

export function episodeState(
  durationMs: number,
  positionMs: number,
): EpisodeState {
  if (positionMs <= 0)
    return {
      kind: "new",
      percent: 0,
      label:
        durationMs > 0 ? `New · ${longDuration(durationMs / 1000)}` : "New",
    };
  if (durationMs > 0 && positionMs >= durationMs - FINISHED_MARGIN_MS)
    return { kind: "played", percent: 100, label: "Played" };
  const left = durationMs > 0 ? durationMs - positionMs : 0;
  return {
    kind: "remaining",
    percent: durationMs > 0 ? (positionMs / durationMs) * 100 : 0,
    label: left > 0 ? `${longDuration(left / 1000)} left` : "In progress",
  };
}
